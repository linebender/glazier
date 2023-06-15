use glazier::{Application, KeyEvent, Region, Scalable, WinHandler, WindowHandle};
use keyboard_types::Key;
use parley::{FontContext, Layout};
use std::any::Any;
use vello::util::{RenderContext, RenderSurface};
use vello::Renderer;
use vello::{
    glyph::{
        pinot::{types::Tag, FontRef},
        GlyphContext,
    },
    kurbo::{Affine, Point, Rect},
    peniko::{Brush, Color, Fill},
    Scene, SceneBuilder,
};

fn main() {
    let app = Application::new().unwrap();
    let window = glazier::WindowBuilder::new(app.clone())
        .size((640., 480.).into())
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
    font_context: FontContext,
    fullscreen: bool,
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
            fullscreen: false,
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
            self.surface = Some(pollster::block_on(self.render.create_surface(
                &self.handle,
                width,
                height,
            )));
        }

        let mut sb = SceneBuilder::for_scene(&mut self.scene);
        let rect = Rect::from_origin_size(Point::new(0.0, 0.0), (width.into(), height.into()));
        sb.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            &Brush::Solid(Color::WHITE_SMOKE),
            None,
            &rect,
        );

        let mut lcx = parley::LayoutContext::new();
        let mut layout_builder =
            lcx.ranged_builder(&mut self.font_context, "Press f to toggle fullscreen", 1.0);
        let mut layout = layout_builder.build();
        layout.break_all_lines(None, parley::layout::Alignment::Start);
        render_text(&mut sb, Affine::IDENTITY, &layout);

        if let Some(surface) = self.surface.as_mut() {
            if surface.config.width != width || surface.config.height != height {
                self.render.resize_surface(surface, width, height);
            }
            let surface_texture = surface.surface.get_current_texture().unwrap();
            let dev_id = surface.dev_id;
            let device = &self.render.devices[dev_id].device;
            let queue = &self.render.devices[dev_id].queue;
            self.renderer
                .get_or_insert_with(|| Renderer::new(device).unwrap())
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

    fn key_up(&mut self, event: KeyEvent) {
        if event.key == Key::Character("f".into()) {
            self.fullscreen ^= true;
            self.handle.set_fullscreen(self.fullscreen);
        }
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
