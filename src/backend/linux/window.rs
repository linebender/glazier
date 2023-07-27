use kurbo::{Insets, Point, Rect, Size};
use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::time::Instant;

#[cfg(feature = "wayland")]
use crate::backend::wayland;
#[cfg(feature = "x11")]
use crate::backend::x11;
use crate::{
    text::Event, Cursor, CursorDesc, Error, FileDialogOptions, FileDialogToken, IdleToken, Scale,
    TextFieldToken, TimerToken, WinHandler, WindowLevel, WindowState,
};

use super::{application::Application, menu::Menu};

#[derive(Clone, PartialEq, Eq)]
pub enum CustomCursor {
    #[cfg(feature = "x11")]
    X11(x11::window::CustomCursor),
    #[cfg(feature = "wayland")]
    Wayland(wayland::window::CustomCursor),
}

impl CustomCursor {
    #[cfg(feature = "x11")]
    pub(crate) fn unwrap_x11(&self) -> &x11::window::CustomCursor {
        match self {
            CustomCursor::X11(it) => it,
            #[cfg(feature = "wayland")]
            CustomCursor::Wayland(_) => panic!("Must use an X11 custom cursor here"),
        }
    }
}

pub(crate) enum WindowBuilder {
    #[cfg(feature = "x11")]
    X11(x11::window::WindowBuilder),
    #[cfg(feature = "wayland")]
    Wayland(wayland::window::WindowBuilder),
}

impl WindowBuilder {
    pub fn new(app: Application) -> Self {
        match app {
            #[cfg(feature = "x11")]
            Application::X11(app) => WindowBuilder::X11(x11::window::WindowBuilder::new(app)),
            #[cfg(feature = "wayland")]
            Application::Wayland(app) => {
                WindowBuilder::Wayland(wayland::window::WindowBuilder::new(app))
            }
        }
    }

    pub fn handler(mut self, handler: Box<dyn WinHandler>) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.handler(handler)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.handler(handler)),
        };
        self
    }

    pub fn size(mut self, size: Size) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.size(size)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.size(size)),
        };
        self
    }

    pub fn min_size(mut self, size: Size) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.min_size(size)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.min_size(size)),
        };
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.resizable(resizable)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.resizable(resizable)),
        };
        self
    }

    pub fn show_titlebar(mut self, show_titlebar: bool) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.show_titlebar(show_titlebar)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => {
                WindowBuilder::Wayland(builder.show_titlebar(show_titlebar))
            }
        };
        self
    }

    pub fn transparent(mut self, transparent: bool) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.transparent(transparent)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => {
                WindowBuilder::Wayland(builder.transparent(transparent))
            }
        };
        self
    }

    pub fn position(mut self, position: Point) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.position(position)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.position(position)),
        };
        self
    }

    pub fn level(mut self, level: WindowLevel) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.level(level)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.level(level)),
        };
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.title(title)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.title(title)),
        };
        self
    }

    pub fn menu(mut self, menu: Menu) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => match menu {
                super::menu::Menu::X11(menu) => WindowBuilder::X11(builder.menu(menu)),
                #[cfg(feature = "wayland")]
                super::menu::Menu::Wayland(_) => WindowBuilder::X11(builder),
            },
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => match menu {
                #[cfg(feature = "x11")]
                super::menu::Menu::X11(_) => WindowBuilder::Wayland(builder),
                super::menu::Menu::Wayland(menu) => WindowBuilder::Wayland(builder.menu(menu)),
            },
        };
        self
    }

    pub fn window_state(mut self, state: WindowState) -> Self {
        self = match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.window_state(state)),
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.window_state(state)),
        };
        self
    }

    pub fn build(self) -> Result<WindowHandle, Error> {
        match self {
            #[cfg(feature = "x11")]
            WindowBuilder::X11(builder) => {
                builder.build().map(WindowHandle::X11).map_err(Into::into)
            }
            #[cfg(feature = "wayland")]
            WindowBuilder::Wayland(builder) => builder
                .build()
                .map(WindowHandle::Wayland)
                .map_err(Into::into),
        }
    }
}

#[derive(Clone)]
pub enum IdleHandle {
    #[cfg(feature = "x11")]
    X11(x11::window::IdleHandle),
    #[cfg(feature = "wayland")]
    Wayland(wayland::window::IdleHandle),
}

impl IdleHandle {
    pub fn add_idle_callback<F>(&self, callback: F)
    where
        F: FnOnce(&mut dyn WinHandler) + Send + 'static,
    {
        match self {
            #[cfg(feature = "x11")]
            IdleHandle::X11(idle) => {
                idle.add_idle_callback(callback);
            }
            #[cfg(feature = "wayland")]
            IdleHandle::Wayland(idle) => {
                idle.add_idle_callback(callback);
            }
        }
    }

    pub fn add_idle_token(&mut self, token: IdleToken) {
        match self {
            #[cfg(feature = "x11")]
            IdleHandle::X11(idle) => {
                idle.add_idle_token(token);
            }
            #[cfg(feature = "wayland")]
            IdleHandle::Wayland(idle) => {
                idle.add_idle_token(token);
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum WindowHandle {
    #[cfg(feature = "x11")]
    X11(x11::window::WindowHandle),
    #[cfg(feature = "wayland")]
    Wayland(wayland::window::WindowHandle),
    None,
}

impl Default for WindowHandle {
    fn default() -> Self {
        Self::None
    }
}
#[cfg(feature = "wayland")]
impl From<wayland::window::WindowHandle> for crate::WindowHandle {
    fn from(value: wayland::window::WindowHandle) -> Self {
        Self(WindowHandle::Wayland(value))
    }
}

#[cfg(feature = "x11")]
impl From<x11::window::WindowHandle> for crate::WindowHandle {
    fn from(value: x11::window::WindowHandle) -> Self {
        Self(WindowHandle::X11(value))
    }
}

impl WindowHandle {
    // #[cfg(feature = "wayland")]
    // /// Assume that this WindowHandle is from Wayland
    // pub(crate) fn unwrap_wayland(&self) -> &wayland::window::WindowHandle {
    //     match self {
    //         WindowHandle::Wayland(it) => it,
    //         _ => unreachable!("Must use a wayland window handle"),
    //     }
    // }
    #[cfg(feature = "x11")]
    /// Assume that this WindowHandle is from X11
    pub(crate) fn unwrap_x11(&self) -> &x11::window::WindowHandle {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(it) => it,
            _ => unreachable!("Must use an x11 window handle"),
        }
    }
}

impl WindowHandle {
    pub fn show(&self) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.show();
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.show();
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn close(&self) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.close();
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.close();
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn resizable(&self, resizable: bool) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.resizable(resizable);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.resizable(resizable);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn set_window_state(&mut self, state: WindowState) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.set_window_state(state);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.set_window_state(state);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn get_window_state(&self) -> WindowState {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.get_window_state(),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.get_window_state(),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn handle_titlebar(&self, val: bool) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.handle_titlebar(val);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.handle_titlebar(val);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn show_titlebar(&self, show_titlebar: bool) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.show_titlebar(show_titlebar);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.show_titlebar(show_titlebar);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn set_position(&self, position: Point) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.set_position(position);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.set_position(position);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn get_position(&self) -> Point {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.get_position(),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.get_position(),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn content_insets(&self) -> Insets {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.content_insets(),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.content_insets(),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn set_size(&self, size: Size) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.set_size(size);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.set_size(size);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn get_size(&self) -> Size {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.get_size(),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.get_size(),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn bring_to_front_and_focus(&self) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.bring_to_front_and_focus();
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.bring_to_front_and_focus();
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn request_anim_frame(&self) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.request_anim_frame();
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.request_anim_frame();
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn invalidate(&self) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.invalidate();
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.invalidate();
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn invalidate_rect(&self, rect: Rect) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.invalidate_rect(rect);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.invalidate_rect(rect);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn set_title(&self, title: &str) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.set_title(title);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.set_title(title);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn set_menu(&self, menu: Menu) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                match menu {
                    super::menu::Menu::X11(menu) => {
                        handle.set_menu(menu);
                    }
                    #[cfg(feature = "wayland")]
                    super::menu::Menu::Wayland(_) => {}
                };
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                match menu {
                    #[cfg(feature = "x11")]
                    super::menu::Menu::X11(_) => {}
                    super::menu::Menu::Wayland(menu) => {
                        handle.set_menu(menu);
                    }
                };
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn add_text_field(&self) -> TextFieldToken {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.add_text_field(),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.add_text_field(),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn remove_text_field(&self, token: TextFieldToken) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.remove_text_field(token);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.remove_text_field(token);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn set_focused_text_field(&self, active_field: Option<TextFieldToken>) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.set_focused_text_field(active_field);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.set_focused_text_field(active_field);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn update_text_field(&self, token: TextFieldToken, update: Event) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.update_text_field(token, update);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.update_text_field(token, update);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn request_timer(&self, deadline: Instant) -> TimerToken {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.request_timer(deadline),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.request_timer(deadline),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn set_cursor(&mut self, cursor: &Cursor) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                handle.set_cursor(cursor);
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                handle.set_cursor(cursor);
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn make_cursor(&self, desc: &CursorDesc) -> Option<Cursor> {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.make_cursor(desc),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.make_cursor(desc),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn open_file(&mut self, options: FileDialogOptions) -> Option<FileDialogToken> {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.open_file(options),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.open_file(options),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn save_as(&mut self, options: FileDialogOptions) -> Option<FileDialogToken> {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.save_as(options),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.save_as(options),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn show_context_menu(&self, menu: Menu, pos: Point) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => {
                match menu {
                    super::menu::Menu::X11(menu) => {
                        handle.show_context_menu(menu, pos);
                    }
                    #[cfg(feature = "wayland")]
                    super::menu::Menu::Wayland(_) => {}
                };
            }
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => {
                match menu {
                    #[cfg(feature = "x11")]
                    super::menu::Menu::X11(_) => {}
                    super::menu::Menu::Wayland(menu) => {
                        handle.show_context_menu(menu, pos);
                    }
                };
            }
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn get_idle_handle(&self) -> Option<IdleHandle> {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.get_idle_handle().map(IdleHandle::X11),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.get_idle_handle().map(IdleHandle::Wayland),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    pub fn get_scale(&self) -> Result<Scale, Error> {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.get_scale().map_err(Into::into),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.get_scale().map_err(Into::into),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }

    #[cfg(feature = "accesskit")]
    pub fn update_accesskit_if_active(
        &self,
        update_factory: impl FnOnce() -> accesskit::TreeUpdate,
    ) {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.update_accesskit_if_active(update_factory),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.update_accesskit_if_active(update_factory),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }
}

unsafe impl HasRawWindowHandle for WindowHandle {
    fn raw_window_handle(&self) -> RawWindowHandle {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.raw_window_handle(),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.raw_window_handle(),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }
}

unsafe impl HasRawDisplayHandle for WindowHandle {
    fn raw_display_handle(&self) -> RawDisplayHandle {
        match self {
            #[cfg(feature = "x11")]
            WindowHandle::X11(handle) => handle.raw_display_handle(),
            #[cfg(feature = "wayland")]
            WindowHandle::Wayland(handle) => handle.raw_display_handle(),
            WindowHandle::None => panic!("Used an uninitialised WindowHandle"),
        }
    }
}
