//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::cell::RefCell;
use std::os::raw::c_void;
use std::rc::Weak;
use std::sync::mpsc::{self, Sender};

use kurbo_0_9::{Rect, Size};
use smithay_client_toolkit::compositor::CompositorHandler;
use smithay_client_toolkit::reexports::calloop::{channel, LoopHandle};
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::{protocol, Connection, Proxy, QueueHandle};
use smithay_client_toolkit::shell::xdg::window::{
    Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::{delegate_compositor, delegate_xdg_shell, delegate_xdg_window};
use thiserror::Error;
use tracing::{self, warn};
use wayland_backend::client::ObjectId;

use crate::window::{IdleToken, Region, Scalable, Scale};
use crate::{PlatformHandler, WindowDescription};

use super::input::{
    input_state, SeatName, TextFieldChange, TextInputProperties, WeakTextInputCell,
};
use super::{ActiveAction, IdleAction, WaylandPlatform, WaylandState};

#[derive(Clone)]
pub struct WindowHandle {
    idle_sender: Sender<IdleAction>,
    loop_sender: channel::Sender<ActiveAction>,
    properties: Weak<RefCell<WindowProperties>>,
    text: WeakTextInputCell,
    // Safety: Points to a wl_display instance
    raw_display_handle: Option<*mut c_void>,
}

#[derive(Error, Debug)]
pub enum BackendWindowCreationError {}

// impl WindowHandle {
//     fn id(&self) -> WindowId {
//         let props = self.properties();
//         let props = props.borrow();
//         WindowId::new(&props.wayland_window)
//     }

//     fn defer(&self, action: WindowAction) {
//         self.loop_sender
//             .send(ActiveAction::Window(self.id(), action))
//             .expect("Running on a window should only occur whilst application is active")
//     }

//     fn properties(&self) -> Rc<RefCell<WindowProperties>> {
//         self.properties.upgrade().unwrap()
//     }

//     pub fn show(&self) {
//         tracing::debug!("show initiated");
//         let props = self.properties();
//         let props = props.borrow();
//         // TODO: Is this valid?
//         props.wayland_window.commit();
//     }

//     pub fn resizable(&self, _resizable: bool) {
//         tracing::warn!("resizable is unimplemented on wayland");
//         // TODO: If we are using fallback decorations, we should be able to disable
//         // dragging based resizing
//     }

//     pub fn show_titlebar(&self, show_titlebar: bool) {
//         tracing::info!("show_titlebar is implemented on a best-effort basis on wayland");
//         // TODO: Track this into the fallback decorations when we add those
//         let props = self.properties();
//         let props = props.borrow();
//         if show_titlebar {
//             props
//                 .wayland_window
//                 .request_decoration_mode(Some(DecorationMode::Server))
//         } else {
//             props
//                 .wayland_window
//                 .request_decoration_mode(Some(DecorationMode::Client))
//         }
//     }

//     pub fn set_position(&self, _position: Point) {
//         tracing::warn!("set_position is unimplemented on wayland");
//         // TODO: Use the KDE plasma extensions for this if available
//         // TODO: Use xdg_positioner if this is a child window
//     }

//     pub fn get_position(&self) -> Point {
//         tracing::warn!("get_position is unimplemented on wayland");
//         Point::ZERO
//     }

//     pub fn content_insets(&self) -> Insets {
//         // I *think* wayland surfaces don't care about content insets
//         // That is, all decorations (to confirm: even client side?) are 'outsets'
//         Insets::from(0.)
//     }

//     pub fn set_size(&self, size: Size) {
//         let props = self.properties();
//         props.borrow_mut().requested_size = Some(size);

//         // We don't need to tell the server about changing the size - so long as the size of the surface gets changed properly
//         // So, all we need to do is to tell the handler about this change (after caching it here)
//         // We must defer this, because we're probably in the handler
//         self.defer(WindowAction::ResizeRequested);
//     }

//     pub fn get_size(&self) -> Size {
//         let props = self.properties();
//         let props = props.borrow();
//         props.current_size
//     }

//     pub fn set_window_state(&mut self, state: glazier::WindowState) {
//         let props = self.properties();
//         let props = props.borrow();
//         match state {
//             glazier::WindowState::Maximized => props.wayland_window.set_maximized(),
//             glazier::WindowState::Minimized => props.wayland_window.set_minimized(),
//             // TODO: I don't think we can do much better than this - we can't unset being minimised
//             glazier::WindowState::Restored => props.wayland_window.unset_maximized(),
//         }
//     }

//     pub fn get_window_state(&self) -> glazier::WindowState {
//         // We can know if we're maximised or restored, but not if minimised
//         tracing::warn!("get_window_state is unimplemented on wayland");
//         glazier::WindowState::Maximized
//     }

//     pub fn handle_titlebar(&self, _val: bool) {
//         tracing::warn!("handle_titlebar is unimplemented on wayland");
//     }

//     /// Close the window.
//     pub fn close(&self) {
//         self.defer(WindowAction::Close)
//     }

//     /// Bring this window to the front of the window stack and give it focus.
//     pub fn bring_to_front_and_focus(&self) {
//         tracing::warn!("unimplemented bring_to_front_and_focus initiated");
//     }

//     /// Request a new paint, but without invalidating anything.
//     pub fn request_anim_frame(&self) {
//         let props = self.properties();
//         let mut props = props.borrow_mut();
//         props.will_repaint = true;
//         if !props.pending_frame_callback {
//             drop(props);
//             self.defer(WindowAction::AnimationRequested);
//         }
//     }

//     /// Request invalidation of the entire window contents.
//     pub fn invalidate(&self) {
//         self.request_anim_frame();
//     }

//     /// Request invalidation of one rectangle, which is given in display points relative to the
//     /// drawing area.
//     pub fn invalidate_rect(&self, _rect: Rect) {
//         todo!()
//     }

//     pub fn add_text_field(&self) -> TextFieldToken {
//         TextFieldToken::next()
//     }

//     pub fn remove_text_field(&self, token: TextFieldToken) {
//         let props_cell = self.text.upgrade().unwrap();
//         let mut props = props_cell.get();
//         let mut updated = false;
//         if props.active_text_field.is_some_and(|it| it == token) {
//             props.active_text_field = None;
//             props.active_text_field_updated = true;
//             updated = true;
//         }
//         if props.next_text_field.is_some_and(|it| it == token) {
//             props.next_text_field = None;
//             updated = true;
//         }

//         if updated {
//             props_cell.set(props);

//             self.defer(WindowAction::TextField(TextFieldChange));
//         }
//     }

//     pub fn set_focused_text_field(&self, active_field: Option<TextFieldToken>) {
//         let props_cell = self.text.upgrade().unwrap();
//         let mut props = props_cell.get();
//         props.next_text_field = active_field;
//         props_cell.set(props);

//         self.defer(WindowAction::TextField(TextFieldChange));
//     }

//     pub fn update_text_field(&self, token: TextFieldToken, update: Event) {
//         let props_cell = self.text.upgrade().unwrap();
//         let mut props = props_cell.get();
//         if props.active_text_field.is_some_and(|it| it == token) {
//             match update {
//                 Event::LayoutChanged => props.active_text_layout_changed = true,
//                 Event::SelectionChanged | Event::Reset => props.active_text_field_updated = true,
//                 _ => {}
//             }
//             props_cell.set(props);
//             self.defer(WindowAction::TextField(TextFieldChange));
//         }
//     }

//     pub fn request_timer(&self, deadline: std::time::Instant) -> TimerToken {
//         let props = self.properties();
//         let props = props.borrow();
//         let window_id = WindowId::new(&props.wayland_window);
//         let token = TimerToken::next();
//         props
//             .loop_handle
//             .insert_source(
//                 Timer::from_deadline(deadline),
//                 move |_deadline, _, state| {
//                     let window = state.windows.get_mut(&window_id);
//                     if let Some(window) = window {
//                         window.handler.timer(token);
//                     }
//                     // In theory, we could get the `timer` request to give us a new deadline
//                     TimeoutAction::Drop
//                 },
//             )
//             .expect("adding a Timer to the calloop event loop is infallible");
//         token
//     }

//     pub fn set_cursor(&mut self, _cursor: &Cursor) {
//         tracing::warn!("unimplemented set_cursor called")
//     }

//     pub fn make_cursor(&self, _desc: &CursorDesc) -> Option<Cursor> {
//         tracing::warn!("unimplemented make_cursor initiated");
//         None
//     }

//     pub fn open_file(&mut self, _options: FileDialogOptions) -> Option<FileDialogToken> {
//         tracing::warn!("unimplemented open_file");
//         None
//     }

//     pub fn save_as(&mut self, _options: FileDialogOptions) -> Option<FileDialogToken> {
//         tracing::warn!("unimplemented save_as");
//         None
//     }

//     /// Get a handle that can be used to schedule an idle task.
//     pub fn get_idle_handle(&self) -> Option<IdleHandle> {
//         Some(IdleHandle {
//             idle_sender: self.idle_sender.clone(),
//             window: self.id(),
//         })
//     }

//     /// Get the `Scale` of the window.
//     pub fn get_scale(&self) -> Result<Scale, ShellError> {
//         let props = self.properties();
//         let props = props.borrow();
//         Ok(props.current_scale)
//     }

//     pub fn set_title(&self, title: &str) {
//         let props = self.properties();
//         let props = props.borrow();
//         props.wayland_window.set_title(title)
//     }

//     #[cfg(feature = "accesskit")]
//     pub fn update_accesskit_if_active(
//         &self,
//         _update_factory: impl FnOnce() -> accesskit::TreeUpdate,
//     ) {
//         // AccessKit doesn't yet support this backend.
//     }
// }

impl PartialEq for WindowHandle {
    fn eq(&self, rhs: &Self) -> bool {
        self.properties.ptr_eq(&rhs.properties)
    }
}

impl Eq for WindowHandle {}

impl Default for WindowHandle {
    fn default() -> WindowHandle {
        // Make fake channels, to work around WindowHandle being default
        let (idle_sender, _) = mpsc::channel();
        let (loop_sender, _) = channel::channel();
        // TODO: Why is this Default?
        WindowHandle {
            properties: Weak::new(),
            raw_display_handle: None,
            idle_sender,
            loop_sender,
            text: Weak::default(),
        }
    }
}

// TODO: Port
// unsafe impl HasRawWindowHandle for WindowHandle {
//     fn raw_window_handle(&self) -> RawWindowHandle {
//         let mut handle = WaylandWindowHandle::empty();
//         let props = self.properties();
//         handle.surface = props.borrow().wayland_window.wl_surface().id().as_ptr() as *mut _;
//         RawWindowHandle::Wayland(handle)
//     }
// }

// unsafe impl HasRawDisplayHandle for WindowHandle {
//     fn raw_display_handle(&self) -> RawDisplayHandle {
//         let mut handle = WaylandDisplayHandle::empty();
//         handle.display = self
//             .raw_display_handle
//             .expect("Window can only be created with a valid display pointer");
//         RawDisplayHandle::Wayland(handle)
//     }
// }

#[derive(Clone, PartialEq, Eq)]
pub struct CustomCursor;

impl WaylandState {
    pub(crate) fn new_window(&mut self, mut desc: WindowDescription) -> crate::WindowId {
        let window_id = desc.assign_id();
        let surface = self.compositor_state.create_surface(&self.wayland_queue);
        let wayland_window = self.xdg_shell_state.create_window(
            surface,
            // Request server decorations, because we don't yet do client decorations properly
            WindowDecorations::RequestServer,
            &self.wayland_queue,
        );
        wayland_window.set_title(desc.title);
        // TODO: Pass this down
        wayland_window.set_app_id("org.linebender.glazier.user_app");
        // TODO: Convert properly, set all properties
        // wayland_window.set_min_size(self.min_size);
        let properties = WindowProperties {
            configure: None,
            requested_size: None,
            // This is just used as the default sizes, as we don't call `size` until the requested size is used
            current_size: Size::new(600., 800.),
            current_scale: Scale::new(1., 1.), // TODO: NaN? - these values should (must?) not be used
            wayland_window,
            wayland_queue: self.wayland_queue,
            loop_handle: self.loop_handle,
            will_repaint: false,
            pending_frame_callback: false,
            configured: false,
        };

        let text = TextInputProperties {
            active_text_field: None,
            next_text_field: None,
            active_text_field_updated: false,
            active_text_layout_changed: false,
        };
        self.surface_to_window.insert(surface, window_id);
        self.windows.insert(
            window_id,
            WaylandWindowState {
                properties,
                text_input_seat: None,
                text,
            },
        );
        window_id
    }
}

/// The state associated with each window, stored in [`WaylandState`]
pub(super) struct WaylandWindowState {
    // TODO: This refcell is too strong - most of the fields can just be Cells
    properties: WindowProperties,
    text_input_seat: Option<SeatName>,
    pub text: TextInputProperties,
}

struct WindowProperties {
    // Requested size is used in configure, if it's supported
    requested_size: Option<Size>,

    configure: Option<WindowConfigure>,
    // The dimensions of the surface we reported to the handler, and so report in get_size()
    // Wayland gives strong deference to the application on surface size
    // so, for example an application using wgpu could have the surface configured to be a different size
    current_size: Size,
    current_scale: Scale,
    // The underlying wayland Window
    // The way to close this Window is to drop the handle
    // We make this the only handle, so we can definitely drop it
    wayland_window: Window,
    wayland_queue: QueueHandle<WaylandPlatform>,
    loop_handle: LoopHandle<'static, WaylandPlatform>,

    /// Wayland requires frame (throttling) callbacks be requested *before* running commit.
    /// However, user code controls when commit is called (generally through wgpu's
    /// `present` in `paint`).
    /// To allow using the frame throttling hints properly we:
    /// - Always request a throttling hint before `paint`ing
    /// - Only action that hint if a request_anim_frame (or equivalent) was called
    /// - If there is no running hint, manually run this process when calling request_anim_frame
    will_repaint: bool,
    /// Whether a `frame` callback has been skipped
    /// If this is false, and painting is requested, we need to manually run our own painting
    pending_frame_callback: bool,
    // We can't draw before being configured
    configured: bool,
}

impl WindowProperties {
    /// Calculate the size that this window should be, given the current configuration
    /// Called in response to a configure event or a resize being requested
    ///
    /// Returns the size which should be passed to [`WinHandler::size`].
    /// This is also set as self.current_size
    fn calculate_size(&mut self) -> Size {
        // We consume the requested size, as that is considered to be a one-shot affair
        // Without doing so, the window would never be resizable
        //
        // TODO: Is this what we want?
        let configure = self.configure.as_ref().unwrap();
        let requested_size = self.requested_size.take();
        if let Some(requested_size) = requested_size {
            if !configure.is_maximized() && !configure.is_resizing() {
                let requested_size_absolute = requested_size.to_px(self.current_scale);
                if let Some((x, y)) = configure.suggested_bounds {
                    if requested_size_absolute.width < x as f64
                        && requested_size_absolute.height < y as f64
                    {
                        self.current_size = requested_size;
                        return self.current_size;
                    }
                } else {
                    self.current_size = requested_size;
                    return self.current_size;
                }
            }
        }
        let current_size_absolute = self.current_size.to_dp(self.current_scale);
        let new_width = configure
            .new_size
            .0
            .map_or(current_size_absolute.width, |it| it.get() as f64);
        let new_height = configure
            .new_size
            .1
            .map_or(current_size_absolute.height, |it| it.get() as f64);
        let new_size_absolute = Size {
            height: new_height,
            width: new_width,
        };

        self.current_size = new_size_absolute.to_dp(self.current_scale);
        self.current_size
    }
}

/// The context do_paint is called in
enum PaintContext {
    /// Painting occurs during a `frame` callback and finished, we know that there are no more frame callbacks
    Frame,
    Requested,
    Configure,
}

impl WaylandWindowState {
    fn do_paint(&mut self, force: bool, context: PaintContext) {
        {
            let mut props = self.properties;
            if matches!(context, PaintContext::Frame) {
                props.pending_frame_callback = false;
            }
            if matches!(context, PaintContext::Requested) && props.pending_frame_callback && !force
            {
                // We'll handle this in the frame callback, when that occurs.
                // This ensures throttling is respected
                // This also prevents a hang on startup, although the reason for that occuring isn't clear
                return;
            }
            if !props.configured || (!props.will_repaint && !force) {
                return;
            }
            props.will_repaint = false;
            // If there is not a frame callback in flight, we request it here
            // This branch could be skipped e.g. on `configure`, which ignores frame throttling hints and
            // always paints eagerly, even if there is a frame callback running
            // TODO: Is that the semantics we want?
            if !props.pending_frame_callback {
                props.pending_frame_callback = true;
                let surface = props.wayland_window.wl_surface();
                surface.frame(&props.wayland_queue.clone(), surface.clone());
            }
        }
        todo!("HANDLER");
        // self.prepare_paint();
        // TODO: Apply invalid properly
        // When forcing, should mark the entire region as damaged
        let mut region = Region::EMPTY;
        {
            let props = self.properties;
            let size = props.current_size.to_dp(props.current_scale);
            region.add_rect(Rect {
                x0: 0.0,
                y0: 0.0,
                x1: size.width,
                y1: size.height,
            });
        }
        todo!("HANDLER");
        // self.handler.paint(&region);
    }

    pub(super) fn set_input_seat(&mut self, seat: SeatName) {
        assert!(self.text_input_seat.is_none());
        self.text_input_seat = Some(seat);
    }
    pub(super) fn remove_input_seat(&mut self, seat: SeatName) {
        assert_eq!(self.text_input_seat, Some(seat));
        self.text_input_seat = None;
    }
}

delegate_xdg_shell!(WaylandPlatform);
delegate_xdg_window!(WaylandPlatform);

delegate_compositor!(WaylandPlatform);

impl CompositorHandler for WaylandPlatform {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &protocol::wl_surface::WlSurface,
        // TODO: Support the fractional-scaling extension instead
        // This requires an update in client-toolkit and wayland-protocols
        new_factor: i32,
    ) {
        let Some(window_id) = self.surface_to_window.get(surface) else {
            warn!(
                "Got surface scale factor change (to {new_factor}) for unknown surface {surface:?}"
            );
            return;
        };
        let window = self
            .windows
            .get_mut(window_id)
            .expect("Should only get events for non-dropped windows");
        let factor = f64::from(new_factor);
        let scale = Scale::new(factor, factor);

        let mut props = window.properties;
        // TODO: Effectively, we need to re-evaluate the size calculation
        // That means we need to cache the WindowConfigure or (mostly) equivalent
        let cur_size_raw: Size = props.current_size.to_px(props.current_scale);
        let new_size = cur_size_raw.to_dp(scale);
        props.current_scale = scale;
        props.current_size = new_size;
        self.with_glz(move |handler, glz| {
            let new_size = new_size;
            let new_scale = scale;
            todo!("Handle ")
            // handler.scale(glz, windowscale);
            // handler.size(new_size);
        });
        // TODO: Do we repaint here?
    }

    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &protocol::wl_surface::WlSurface,
        _time: u32,
    ) {
        let Some(window_id) = self.surface_to_window.get(surface) else {
            warn!("Got repaint for unknown surface {surface:?}");
            return;
        };
        let window = self
            .windows
            .get_mut(window_id)
            .expect("Should only get events for non-dropped windows");
        window.do_paint(false, PaintContext::Frame);
    }
}

impl WindowHandler for WaylandPlatform {
    fn request_close(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        wl_window: &smithay_client_toolkit::shell::xdg::window::Window,
    ) {
        let Some(window_id) = self.surface_to_window.get(wl_window.wl_surface()) else {
            warn!("Got request close for unknown window {wl_window:?}");
            return;
        };
        let window = self
            .windows
            .get_mut(window_id)
            .expect("Should only get events for non-dropped windows");
        self.with_glz(|handler, glz| handler.window);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        window: &smithay_client_toolkit::shell::xdg::window::Window,
        configure: smithay_client_toolkit::shell::xdg::window::WindowConfigure,
        _: u32,
    ) {
        let window = if let Some(window) = self.windows.get_mut(&WindowId::new(window)) {
            window
        } else {
            // Using let else here breaks formatting with rustfmt
            tracing::warn!("Received configure event for unknown window");
            return;
        };
        // TODO: Actually use the suggestions from requested_size
        let display_size;
        {
            let mut props = window.properties;
            props.configure = Some(configure);
            display_size = props.calculate_size();
            props.configured = true;
        };
        // window.handler.size(display_size);
        // self.with_glz(|plat, glz| plat.surface_available(glz, win));
        todo!("HANDLER");
        window.do_paint(true, PaintContext::Configure);
    }
}

pub(super) enum WindowAction {
    /// Change the window size, based on `requested_size`
    ///
    /// `requested_size` must be set before this is called
    ResizeRequested,
    /// Close the Window
    Close,
    Create(WaylandWindowState),
    AnimationRequested,
    TextField(TextFieldChange),
}

impl WindowAction {
    pub(super) fn run(self, state: &mut WaylandState, window_id: WindowId) {
        match self {
            WindowAction::ResizeRequested => {
                let Some(window) = state.windows.get_mut(&window_id) else {
                    return;
                };
                let size = {
                    let mut props = window.properties.borrow_mut();
                    props.calculate_size()
                };
                // TODO: Ensure we follow the rules laid out by the compositor in `configure`
                todo!("HANDLER");
                // window.handler.size(size);
                // Force repainting now that the size has changed.
                // TODO: Should this only happen if the size is actually different?
                window.do_paint(true, PaintContext::Requested);
            }
            WindowAction::Close => {
                // Remove the window from tracking
                {
                    let Some(win) = state.windows.remove(&window_id) else {
                        tracing::error!("Tried to close the same window twice");
                        return;
                    };
                    if let Some(seat) = win.text_input_seat {
                        let seat = input_state(&mut state.input_states, seat);
                        seat.window_deleted(&mut state.windows);
                    }
                }
                // We will drop the proper wayland window later when we Drop window.props
                if state.windows.is_empty() {
                    state.loop_signal.stop();
                }
            }
            WindowAction::Create(win_state) => {
                state.windows.insert(window_id, win_state);
            }
            WindowAction::AnimationRequested => {
                let Some(window) = state.windows.get_mut(&window_id) else {
                    return;
                };
                window.do_paint(false, PaintContext::Requested);
            }
            WindowAction::TextField(change) => {
                let Some(props) = state.windows.get_mut(&window_id) else {
                    return;
                };
                let Some(seat) = props.text_input_seat else {
                    return;
                };
                change.apply(
                    input_state(&mut state.input_states, seat),
                    &mut state.windows,
                    &window_id,
                );
            }
        }
    }
}
