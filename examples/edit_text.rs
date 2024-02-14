use std::any::Any;
use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

#[cfg(feature = "accesskit")]
use accesskit::TreeUpdate;
use instant::Duration;
use parley::FontContext;
use tracing_subscriber::EnvFilter;
use unicode_segmentation::GraphemeCursor;
use vello::util::{RenderContext, RenderSurface};
use vello::{
    kurbo::{Affine, Point, Rect},
    peniko::{Brush, Color, Fill},
    RenderParams, RendererOptions, Scene,
};
use vello::{AaSupport, Renderer};

use glazier::kurbo::Size;
use glazier::{
    text::{
        Action, Affinity, Direction, Event, HitTestPoint, InputHandler, Movement, Selection,
        VerticalMovement,
    },
    Application, KeyEvent, Region, Scalable, TextFieldToken, WinHandler, WindowHandle,
};
use glazier::{HotKey, SysMods, TimerToken};

mod common;
use common::text::{self, ParleyBrush};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;
const FONT_SIZE: f32 = 36.0;
const TEXT_X: f64 = 100.0;
const TEXT_Y: f64 = 100.0;
// TODO: We need to get this from the user's system, as most (all?) desktops have a setting for this
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(600);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let app = Application::new().unwrap();
    let window = glazier::WindowBuilder::new(app.clone())
        .resizable(true)
        .size((WIDTH as f64 / 2., HEIGHT as f64 / 2.).into())
        .title("Edit Text - Glazier Example")
        .handler(Box::new(WindowState::new()))
        .build()
        .unwrap();
    window.show();
    app.run(None);
}

struct HotKeys {
    copy: HotKey,
    paste: HotKey,
    select_all: HotKey,
}

impl HotKeys {
    fn new() -> Self {
        HotKeys {
            copy: HotKey::new(SysMods::Cmd, "c"),
            paste: HotKey::new(SysMods::Cmd, "v"),
            select_all: HotKey::new(SysMods::Cmd, "a"),
        }
    }
}

impl Default for HotKeys {
    fn default() -> Self {
        Self::new()
    }
}

struct WindowState {
    handle: WindowHandle,
    render: RenderContext,
    renderer: Option<Renderer>,
    surface: Option<RenderSurface>,
    scene: Scene,
    size: Size,
    document: Rc<RefCell<DocumentState>>,
    text_input_token: Option<TextFieldToken>,
    hotkeys: HotKeys,

    cursor_blink_token: Option<TimerToken>,
    cursor_shown: bool,
}

struct DocumentState {
    text: String,
    selection: Selection,
    composition: Option<Range<usize>>,
    layout: parley::Layout<ParleyBrush>,
    font_context: FontContext,
}

impl DocumentState {
    fn update_selection(&mut self) {
        let next_char_boundary = |mut idx| {
            if idx > self.text.len() {
                panic!("Tried to set selection outside of the string")
            }
            while !self.text.is_char_boundary(idx) {
                idx += 1;
            }
            idx
        };
        self.selection.active = next_char_boundary(self.selection.active);
        self.selection.anchor = next_char_boundary(self.selection.anchor);
    }
}

impl Default for DocumentState {
    fn default() -> Self {
        let mut this = Self {
            text: "hello world".to_string(),
            selection: Default::default(),
            composition: None,
            layout: Default::default(),
            font_context: FontContext::new(),
        };
        this.refresh_layout();
        this
    }
}

impl DocumentState {
    fn refresh_layout(&mut self) {
        let mut lcx = parley::LayoutContext::new();
        let contents = self.text.to_string();
        let mut layout_builder = lcx.ranged_builder(&mut self.font_context, &contents, 1.0);
        layout_builder.push_default(&parley::style::StyleProperty::FontSize(FONT_SIZE));
        layout_builder.push_default(&parley::style::StyleProperty::Brush(ParleyBrush(
            Brush::Solid(Color::rgb8(0, 0, 0)),
        )));
        let mut layout = layout_builder.build();
        layout.break_all_lines(None, parley::layout::Alignment::Start);
        self.layout = layout;
    }
}

struct AppInputHandler {
    state: Rc<RefCell<DocumentState>>,
    window_size: Size,
    window_handle: WindowHandle,
}

impl WindowState {
    pub fn new() -> Self {
        let render = (RenderContext::new()).unwrap();
        let document: Rc<RefCell<DocumentState>> = Default::default();
        Self {
            handle: Default::default(),
            document,
            surface: None,
            render,
            renderer: None,
            scene: Default::default(),
            size: Size::new(800.0, 600.0),
            text_input_token: None,
            hotkeys: Default::default(),
            cursor_blink_token: None,
            cursor_shown: true,
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

        self.render_anim_frame();

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
                use_cpu: false,
                antialiasing_support: AaSupport {
                    area: true,
                    msaa8: false,
                    msaa16: false,
                },
            };
            let render_params = RenderParams {
                base_color: Color::BLACK,
                width,
                height,
                antialiasing_method: vello::AaConfig::Area,
            };
            self.renderer
                .get_or_insert_with(|| Renderer::new(device, renderer_options).unwrap())
                .render_to_surface(device, queue, &self.scene, &surface_texture, &render_params)
                .unwrap();
            surface_texture.present();
            device.poll(wgpu::Maintain::Poll);
        }
    }

    fn render_anim_frame(&mut self) {
        let (height, width) = self.surface_size();
        self.scene.reset();
        let rect =
            Rect::from_origin_size(Point::new(0.0, 0.0), Size::new(height as f64, width as f64));
        self.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            &Brush::Solid(Color::rgb8(255, 255, 255)),
            None,
            &rect,
        );
        let doc = self.document.borrow();
        text::render_text(
            &mut self.scene,
            Affine::translate((TEXT_X, TEXT_Y)),
            &doc.layout,
        );
        if doc.selection.len() > 0 {
            let selection_start_x =
                parley::layout::Cursor::from_position(&doc.layout, doc.selection.min(), true)
                    .offset() as f64
                    + TEXT_X;
            let selection_end_x =
                parley::layout::Cursor::from_position(&doc.layout, doc.selection.max(), true)
                    .offset() as f64
                    + TEXT_X;
            let rect = Rect::from_points(
                Point::new(selection_start_x, TEXT_Y),
                Point::new(selection_end_x, TEXT_Y + FONT_SIZE as f64),
            );
            self.scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                &Brush::Solid(Color::rgba8(0, 0, 255, 100)),
                None,
                &rect,
            );
        }
        if self.cursor_shown {
            let cursor_active_x =
                parley::layout::Cursor::from_position(&doc.layout, doc.selection.active, true)
                    .offset() as f64
                    + TEXT_X;
            let rect = Rect::from_points(
                Point::new(cursor_active_x - 1.0, TEXT_Y),
                Point::new(cursor_active_x + 1.0, TEXT_Y + FONT_SIZE as f64),
            );
            self.scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                &Brush::Solid(Color::BLACK),
                None,
                &rect,
            );
        }
        if let Some(composition) = &doc.composition {
            let composition_start = parley::layout::Cursor::from_position(
                &doc.layout,
                composition.start,
                true,
            )
            .offset() as f64
                + TEXT_X;
            let composition_end =
                parley::layout::Cursor::from_position(&doc.layout, composition.end, true).offset()
                    as f64
                    + TEXT_X;
            let rect = Rect::from_points(
                Point::new(
                    composition_start.min(composition_end - 1.0),
                    TEXT_Y + FONT_SIZE as f64 + 7.0,
                ),
                Point::new(
                    composition_start.max(composition_end + 1.0),
                    TEXT_Y + FONT_SIZE as f64 + 5.0,
                ),
            );
            self.scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                &Brush::Solid(Color::rgba8(0, 0, 255, 100)),
                None,
                &rect,
            );
        }

        self.scene.pop_layer();
    }
}

impl WinHandler for WindowState {
    fn connect(&mut self, handle: &WindowHandle) {
        self.handle = handle.clone();
        let token = self.handle.add_text_field();
        self.text_input_token = Some(token);
        self.handle.set_focused_text_field(Some(token));
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

    fn size(&mut self, size: Size) {
        self.size = size;
    }

    fn request_close(&mut self) {
        self.handle.close();
    }

    fn destroy(&mut self) {
        Application::global().quit();
    }

    fn acquire_input_lock(
        &mut self,
        _token: TextFieldToken,
        _mutable: bool,
    ) -> Box<dyn InputHandler> {
        Box::new(AppInputHandler {
            state: self.document.clone(),
            window_size: self.size,
            window_handle: self.handle.clone(),
        })
    }

    fn release_input_lock(&mut self, _token: TextFieldToken) {
        // Applications appear to only hide the cursor when there has been no change
        // to the text. As a best attempt version of this, show the cursor
        // when an input lock finishes. TODO: Should this only be on a mutable lock?

        // This doesn't work for keypresses which don't do anything to the text (such as pressing escape
        // for example), but this is good enough
        self.cursor_shown = true;
        // Ignore the previous request
        self.cursor_blink_token = Some(self.handle.request_timer(CURSOR_BLINK_INTERVAL));
    }

    fn key_down(&mut self, event: &KeyEvent) -> bool {
        self.schedule_render();
        if self.hotkeys.copy.matches(event) {
            let doc = self.document.borrow_mut();
            let text = &doc.text[doc.selection.range()];
            Application::global().clipboard().put_string(text); // return true prevents the keypress event from being handled as text input

            return true;
        }
        if self.hotkeys.paste.matches(event) {
            println!("Pasting");
            let clipboard_contents = Application::global().clipboard().get_string();
            if let Some(mut contents) = clipboard_contents {
                contents.retain(|c| c != '\n');
                {
                    let mut doc = self.document.borrow_mut();
                    let selection = doc.selection;
                    doc.text.replace_range(selection.range(), &contents);
                    let new_caret_index = selection.min() + contents.len();
                    doc.selection = Selection::caret(new_caret_index);
                    doc.refresh_layout();
                    doc.composition = None;
                }
                // notify the OS that we've updated the selection
                self.handle
                    .update_text_field(self.text_input_token.unwrap(), Event::Reset);

                // repaint window
                self.handle.request_anim_frame();
            }

            return true;
        }
        if self.hotkeys.select_all.matches(event) {
            {
                let mut doc = self.document.borrow_mut();
                doc.selection = Selection::new(0, doc.text.len());
            }
            // notify the OS that we've updated the selection
            self.handle
                .update_text_field(self.text_input_token.unwrap(), Event::SelectionChanged);

            // repaint window
            self.handle.request_anim_frame();

            // return true prevents the keypress event from being handled as text input
            return true;
        }
        false
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
    fn got_focus(&mut self) {
        self.cursor_blink_token = Some(self.handle.request_timer(CURSOR_BLINK_INTERVAL));
        // The text field is always focused in this example, so start with it shown
        self.cursor_shown = true;
    }

    fn lost_focus(&mut self) {
        self.cursor_shown = false;
        // Don't
        self.cursor_blink_token = None;
    }
    fn timer(&mut self, token: TimerToken) {
        if self.cursor_blink_token.is_some_and(|it| it == token) {
            self.cursor_shown = !self.cursor_shown;
            self.cursor_blink_token = Some(self.handle.request_timer(CURSOR_BLINK_INTERVAL));
        }
        // Ignore the token otherwise, as it's been superceded
    }
}

impl InputHandler for AppInputHandler {
    fn selection(&self) -> Selection {
        self.state.borrow().selection
    }
    fn composition_range(&self) -> Option<Range<usize>> {
        self.state.borrow().composition.clone()
    }
    fn set_selection(&mut self, range: Selection) {
        let mut state = self.state.borrow_mut();
        state.selection = range;
        state.update_selection();
        self.window_handle.request_anim_frame();
    }
    fn set_composition_range(&mut self, range: Option<Range<usize>>) {
        self.state.borrow_mut().composition = range;
        self.window_handle.request_anim_frame();
    }
    fn replace_range(&mut self, range: Range<usize>, text: &str) {
        let mut doc = self.state.borrow_mut();
        doc.text.replace_range(range.clone(), text);
        if doc.selection.anchor < range.start && doc.selection.active < range.start {
            // no need to update selection
        } else if doc.selection.anchor > range.end && doc.selection.active > range.end {
            doc.selection.anchor -= range.len();
            doc.selection.active -= range.len();
            doc.selection.anchor += text.len();
            doc.selection.active += text.len();
        } else {
            doc.selection.anchor = range.start + text.len();
            doc.selection.active = range.start + text.len();
        }
        doc.update_selection();
        doc.refresh_layout();
        doc.composition = None;
        self.window_handle.request_anim_frame();
    }
    fn slice(&self, range: Range<usize>) -> Cow<str> {
        self.state.borrow().text[range].to_string().into()
    }
    fn is_char_boundary(&self, i: usize) -> bool {
        self.state.borrow().text.is_char_boundary(i)
    }
    fn len(&self) -> usize {
        self.state.borrow().text.len()
    }
    fn hit_test_point(&self, point: Point) -> HitTestPoint {
        let document = self.state.borrow();
        let cursor = parley::layout::Cursor::from_point(
            &document.layout,
            (point.x - TEXT_X) as f32,
            (point.y - TEXT_Y) as f32,
        );
        let idx = match cursor.is_leading() {
            true => cursor.text_range().start,
            false => cursor.text_range().end,
        };
        HitTestPoint::new(idx, cursor.is_inside())
    }
    fn bounding_box(&self) -> Option<Rect> {
        Some(Rect::new(
            0.0,
            0.0,
            self.window_size.width,
            self.window_size.height,
        ))
    }
    fn slice_bounding_box(&self, range: Range<usize>) -> Option<Rect> {
        let doc = self.state.borrow();
        let range_start = parley::layout::Cursor::from_position(&doc.layout, range.start, true);
        let range_end = parley::layout::Cursor::from_position(&doc.layout, range.end, true);
        Some(Rect::new(
            range_start.offset() as f64 + TEXT_X,
            range_start.baseline() as f64 + TEXT_Y - 30.,
            range_end.offset() as f64 + TEXT_X + 5.0,
            range_end.baseline() as f64 + TEXT_Y,
        ))
    }
    fn line_range(&self, _char_index: usize, _affinity: Affinity) -> Range<usize> {
        // we don't have multiple lines, so no matter the input, output is the whole document
        0..self.state.borrow().text.len()
    }

    fn handle_action(&mut self, action: Action) {
        let handled = apply_default_behavior(self, action);
        println!("action: {:?} handled: {:?}", action, handled);
    }
}

fn apply_default_behavior(handler: &mut AppInputHandler, action: Action) -> bool {
    let is_caret = handler.selection().is_caret();
    match action {
        Action::Move(movement) => {
            let selection = handler.selection();
            let index = if movement_goes_downstream(movement) {
                selection.max()
            } else {
                selection.min()
            };
            let updated_index = if let (false, Movement::Grapheme(_)) = (is_caret, movement) {
                // handle special cases of pressing left/right when the selection is not a caret
                index
            } else {
                match apply_movement(handler, movement, index) {
                    Some(v) => v,
                    None => return false,
                }
            };
            handler.set_selection(Selection::caret(updated_index));
        }
        Action::MoveSelecting(movement) => {
            let mut selection = handler.selection();
            selection.active = match apply_movement(handler, movement, selection.active) {
                Some(v) => v,
                None => return false,
            };
            handler.set_selection(selection);
        }
        Action::SelectAll => {
            let len = handler.len();
            let selection = Selection::new(0, len);
            handler.set_selection(selection);
        }
        Action::Delete(_) if !is_caret => {
            // movement is ignored for non-caret selections
            let selection = handler.selection();
            handler.replace_range(selection.range(), "");
        }
        Action::Delete(movement) => {
            let mut selection = handler.selection();
            selection.active = match apply_movement(handler, movement, selection.active) {
                Some(v) => v,
                None => return false,
            };
            handler.replace_range(selection.range(), "");
        }
        _ => return false,
    }
    true
}

fn movement_goes_downstream(movement: Movement) -> bool {
    match movement {
        Movement::Grapheme(dir) => direction_goes_downstream(dir),
        Movement::Word(dir) => direction_goes_downstream(dir),
        Movement::Line(dir) => direction_goes_downstream(dir),
        Movement::ParagraphEnd => true,
        Movement::Vertical(VerticalMovement::LineDown) => true,
        Movement::Vertical(VerticalMovement::PageDown) => true,
        Movement::Vertical(VerticalMovement::DocumentEnd) => true,
        _ => false,
    }
}

fn direction_goes_downstream(direction: Direction) -> bool {
    match direction {
        Direction::Left => false,
        Direction::Right => true,
        Direction::Upstream => false,
        Direction::Downstream => true,
    }
}

fn apply_movement(edit_lock: &AppInputHandler, movement: Movement, index: usize) -> Option<usize> {
    match movement {
        Movement::Grapheme(dir) => {
            let doc_len = edit_lock.len();
            let mut cursor = GraphemeCursor::new(index, doc_len, true);
            let doc = edit_lock.slice(0..doc_len);
            if direction_goes_downstream(dir) {
                cursor.next_boundary(&doc, 0).unwrap()
            } else {
                cursor.prev_boundary(&doc, 0).unwrap()
            }
        }
        _ => None,
    }
}
