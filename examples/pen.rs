use std::any::Any;
use std::collections::HashMap;
use std::f64::consts::PI;

#[cfg(feature = "accesskit")]
use accesskit::TreeUpdate;
use kurbo::Ellipse;
use vello::util::{RenderContext, RenderSurface};
use vello::Renderer;
use vello::{
    kurbo::{Affine, BezPath, Point, Rect},
    peniko::{Brush, Color, Fill, Stroke},
    RenderParams, RendererOptions, Scene, SceneBuilder,
};

use glazier::kurbo::Size;
use glazier::{
    Application, Cursor, FileDialogToken, FileInfo, IdleToken, KeyEvent, PenInclination,
    PointerEvent, PointerId, PointerType, Region, Scalable, TimerToken, WinHandler, WindowHandle,
};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;

fn main() {
    pretty_env_logger::init();
    let app = Application::new().unwrap();
    let window_handle = glazier::WindowBuilder::new(app.clone())
        .size((WIDTH as f64 / 2.0, HEIGHT as f64 / 2.0).into())
        .handler(Box::new(WindowState::new()))
        .build()
        .unwrap();
    window_handle.show();
    app.run(None);
}

pub struct PenState {
    pos: Point,
    inclination: PenInclination,
    pressure: f64,
}

#[derive(Default)]
pub struct TouchState {
    points: HashMap<PointerId, (BezPath, Color)>,
}

struct WindowState {
    handle: WindowHandle,
    renderer: Option<Renderer>,
    render: RenderContext,
    surface: Option<RenderSurface>,
    scene: Scene,
    size: Size,
    pen_state: Option<PenState>,
    touch_state: TouchState,
    finger_colors: Box<dyn Iterator<Item = Color>>,
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
            size: Size::new(800.0, 600.0),
            pen_state: None,
            touch_state: Default::default(),
            finger_colors: Box::new(
                vec![Color::ORANGE, Color::PURPLE, Color::BLUE, Color::BLACK]
                    .into_iter()
                    .cycle(),
            ),
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

        render_anim_frame(&mut self.scene, self.pen_state.as_ref(), &self.touch_state);

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
        TreeUpdate::default()
    }

    fn idle(&mut self, _: IdleToken) {
        self.render();
        self.schedule_render();
    }

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

    fn pointer_move(&mut self, event: &PointerEvent) {
        self.handle.set_cursor(&Cursor::Arrow);
        match &event.pointer_type {
            PointerType::Pen(info) => {
                self.pen_state = Some(PenState {
                    pressure: info.pressure,
                    inclination: info.inclination,
                    pos: event.pos,
                });
            }
            PointerType::Touch(_) => {
                if let Some((line, _)) = self.touch_state.points.get_mut(&event.pointer_id) {
                    line.line_to(event.pos);
                } else {
                    tracing::warn!("moved an unknown finger");
                }
            }
            _ => {}
        }
    }

    fn pointer_down(&mut self, event: &PointerEvent) {
        if let PointerType::Touch(_) = &event.pointer_type {
            let color = if event.is_primary {
                Color::RED
            } else {
                self.finger_colors.next().unwrap()
            };
            let mut path = BezPath::new();
            path.move_to(event.pos);
            self.touch_state
                .points
                .insert(event.pointer_id, (path, color));
        }
    }

    fn pointer_up(&mut self, event: &PointerEvent) {
        if let PointerType::Touch(_) = &event.pointer_type {
            self.touch_state.points.remove(&event.pointer_id);
        }
    }

    fn pointer_leave(&mut self) {
        self.pen_state = None;
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

pub fn render_anim_frame(
    scene: &mut Scene,
    pen_state: Option<&PenState>,
    touch_state: &TouchState,
) {
    let mut sb = SceneBuilder::for_scene(scene);
    let rect = Rect::from_origin_size(Point::new(0.0, 0.0), (5000.0, 5000.0));
    sb.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        &Brush::Solid(Color::rgb8(128, 128, 128)),
        None,
        &rect,
    );

    if let Some(state) = pen_state {
        let r = (state.pressure + 0.1) * 50.0;
        let shape = Ellipse::new(
            state.pos,
            (r, r * state.inclination.altitude.to_degrees() / 90.0),
            state.inclination.azimuth.to_radians() + PI / 2.0,
        );
        sb.fill(Fill::NonZero, Affine::IDENTITY, Color::BLUE, None, &shape);
    }

    for (path, color) in touch_state.points.values() {
        sb.stroke(&Stroke::default(), Affine::IDENTITY, color, None, path);
    }

    sb.pop_layer();
}
