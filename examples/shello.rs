use glazier::kurbo::Size;
use glazier::{
    Application, Cursor, FileDialogToken, FileInfo, IdleToken, KeyEvent, MouseEvent, Region,
    Scalable, TimerToken, WinHandler, WindowHandle,
};
use parley::{FontContext, Layout};
use std::any::Any;
use vello::util::{RenderContext, RenderSurface};
use vello::Renderer;
use vello::{
    glyph::{
        pinot::{types::Tag, FontRef},
        GlyphContext,
    },
    kurbo::{Affine, PathEl, Point, Rect},
    peniko::{Brush, Color, Fill, Mix, Stroke},
    Scene, SceneBuilder,
};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;

fn main() {
    let app = Application::new().unwrap();
    let mut window_builder = glazier::WindowBuilder::new(app.clone());
    window_builder.resizable(false);
    window_builder.set_size((WIDTH as f64 / 2., HEIGHT as f64 / 2.).into());
    window_builder.set_handler(Box::new(WindowState::new()));
    let window_handle = window_builder.build().unwrap();
    window_handle.show();
    app.run(None);
}

struct WindowState {
    handle: WindowHandle,
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
            render,
            scene: Default::default(),
            font_context: FontContext::new(),
            counter: 0,
            size: Size::new(800.0, 600.0),
        }
    }

    #[cfg(target_os = "macos")]
    fn schedule_render(&self) {
        self.handle
            .get_idle_handle()
            .unwrap()
            .schedule_idle(IdleToken::new(0));
    }

    #[cfg(not(target_os = "macos"))]
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
            self.surface = Some(pollster::block_on(self.render.create_surface(
                &self.handle,
                width,
                height,
            )));
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
            let mut renderer = Renderer::new(device).unwrap();

            renderer
                .render_to_surface(device, queue, &self.scene, &surface_texture, width, height)
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

    fn key_down(&mut self, event: KeyEvent) -> bool {
        println!("keydown: {event:?}");
        false
    }

    fn key_up(&mut self, event: KeyEvent) {
        println!("keyup: {event:?}");
    }

    fn wheel(&mut self, event: &MouseEvent) {
        println!("mouse_wheel {event:?}");
    }

    fn mouse_move(&mut self, _event: &MouseEvent) {
        self.handle.set_cursor(&Cursor::Arrow);
        //println!("mouse_move {event:?}");
    }

    fn mouse_down(&mut self, event: &MouseEvent) {
        println!("mouse_down {event:?}");
    }

    fn mouse_up(&mut self, event: &MouseEvent) {
        println!("mouse_up {event:?}");
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

#[derive(Clone, Debug)]
pub struct ParleyBrush(pub Brush);

impl Default for ParleyBrush {
    fn default() -> ParleyBrush {
        ParleyBrush(Brush::Solid(Color::rgb8(0, 0, 0)))
    }
}

impl PartialEq<ParleyBrush> for ParleyBrush {
    fn eq(&self, _other: &ParleyBrush) -> bool {
        true // FIXME
    }
}

impl parley::style::Brush for ParleyBrush {}

pub fn render_text(builder: &mut SceneBuilder, transform: Affine, layout: &Layout<ParleyBrush>) {
    let mut gcx = GlyphContext::new();
    for line in layout.lines() {
        for glyph_run in line.glyph_runs() {
            let mut x = glyph_run.offset();
            let y = glyph_run.baseline();
            let run = glyph_run.run();
            let font = run.font().as_ref();
            let font_size = run.font_size();
            let font_ref = FontRef {
                data: font.data,
                offset: font.offset,
            };
            let style = glyph_run.style();
            let vars: [(Tag, f32); 0] = [];
            let mut gp = gcx.new_provider(&font_ref, None, font_size, false, vars);
            for glyph in glyph_run.glyphs() {
                if let Some(fragment) = gp.get(glyph.id, Some(&style.brush.0)) {
                    let gx = x + glyph.x;
                    let gy = y - glyph.y;
                    let xform = Affine::translate((gx as f64, gy as f64))
                        * Affine::scale_non_uniform(1.0, -1.0);
                    builder.append(&fragment, Some(transform * xform));
                }
                x += glyph.advance;
            }
        }
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
    render_text(&mut sb, Affine::translate((100.0, 400.0)), &layout);

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
