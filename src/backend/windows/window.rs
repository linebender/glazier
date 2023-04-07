// Copyright 2018 The Druid Authors.
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

//! Creation and management of windows.

#![allow(non_snake_case, clippy::cast_lossless)]

use std::cell::{Cell, RefCell};
use std::mem;
use std::panic::Location;
use std::ptr::{null, null_mut};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(feature = "accesskit")]
use accesskit_windows::{Adapter as AccessKitAdapter, UiaInitMarker};
#[cfg(feature = "accesskit")]
use once_cell::unsync::OnceCell;
use scopeguard::defer;
use tracing::{error, warn};
use winapi::ctypes::{c_int, c_void};
use winapi::shared::minwindef::*;
use winapi::shared::windef::*;
use winapi::shared::winerror::*;
use winapi::um::dwmapi::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute};
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::shellscalingapi::MDT_EFFECTIVE_DPI;
use winapi::um::uxtheme::*;
use winapi::um::wingdi::*;
use winapi::um::winnt::*;
use winapi::um::winuser::*;

use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle, Win32WindowHandle,
    WindowsDisplayHandle,
};

use crate::kurbo::{Insets, Point, Rect, Size, Vec2};

use super::accels::register_accel;
use super::application::Application;
use super::dialog::get_file_dialog_path;
use super::error::Error;
use super::keyboard::{self, KeyboardState};
use super::menu::Menu;
// use super::paint;
use super::timers::TimerSlots;
use super::util::{self, ToWide, OPTIONAL_FUNCTIONS};

use crate::common_util::IdleCallback;
use crate::dialog::{FileDialogOptions, FileDialogType, FileInfo};
use crate::error::Error as ShellError;
use crate::keyboard::{KbKey, KeyState};
use crate::mouse::{Cursor, CursorDesc, MouseButton, MouseButtons, MouseEvent};
use crate::region::Region;
use crate::scale::{Scalable, Scale, ScaledArea};
use crate::text::{simulate_input, Event};
use crate::window;
use crate::window::{
    FileDialogToken, IdleToken, TextFieldToken, TimerToken, WinHandler, WindowLevel,
};

/// The backend target DPI.
///
/// Windows considers 96 the default value which represents a 1.0 scale factor.
pub(crate) const SCALE_TARGET_DPI: f64 = 96.0;

/// Builder abstraction for creating new windows.
pub(crate) struct WindowBuilder {
    app: Application,
    handler: Option<Box<dyn WinHandler>>,
    title: String,
    menu: Option<Menu>,
    present_strategy: PresentStrategy,
    resizable: bool,
    show_titlebar: bool,
    size: Option<Size>,
    transparent: bool,
    min_size: Option<Size>,
    position: Option<Point>,
    level: Option<WindowLevel>,
    state: window::WindowState,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
/// It's very tricky to get smooth dynamics (especially resizing) and
/// good performance on Windows. This setting lets clients experiment
/// with different strategies.
#[allow(dead_code)]
pub enum PresentStrategy {
    /// Corresponds to the swap effect DXGI_SWAP_EFFECT_SEQUENTIAL. It
    /// is compatible with GDI (such as menus), but is not the best in
    /// performance.
    ///
    /// In earlier testing, it exhibited diagonal banding artifacts (most
    /// likely because of bugs in Nvidia Optimus configurations) and did
    /// not do incremental present, but in more recent testing, at least
    /// incremental present seems to work fine.
    ///
    /// Also note, this swap effect is not compatible with DX12.
    #[default]
    Sequential,

    /// Corresponds to the swap effect DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL.
    /// In testing, it seems to perform well, but isn't compatible with
    /// GDI. Resize can probably be made to work reasonably smoothly with
    /// additional synchronization work, but has some artifacts.
    Flip,

    /// Corresponds to the swap effect DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL
    /// but with a redirection surface for GDI compatibility. Resize is
    /// very laggy and artifacty.
    FlipRedirect,
}

/// An enumeration of operations that might need to be deferred until the `WinHandler` is dropped.
///
/// We work hard to avoid calling into `WinHandler` re-entrantly. Since we use
/// the system's event loop, and since the `WinHandler` gets a `WindowHandle` to use, this implies
/// that none of the `WindowHandle`'s methods can return control to the system's event loop
/// (because if it did, the system could call back into glazier with some mouse event, and then
/// we'd try to call the `WinHandler` again).
///
/// The solution is that for every `WindowHandle` method that *wants* to return control to the
/// system's event loop, instead of doing that we queue up a deferrred operation and return
/// immediately. The deferred operations will run whenever the currently running `WinHandler`
/// method returns.
///
/// An example call trace might look like:
/// 1. the system hands a mouse click event to glazier
/// 2. glazier calls `WinHandler::mouse_up`
/// 3. after some processing, the `WinHandler` calls `WindowHandle::save_as`, which schedules a
///   deferred op and returns immediately
/// 4. after some more processing, `WinHandler::mouse_up` returns
/// 5. glazier displays the "save as" dialog that was requested in step 3.
enum DeferredOp {
    SaveAs(FileDialogOptions, FileDialogToken),
    Open(FileDialogOptions, FileDialogToken),
    ContextMenu(Menu, Point),
    ShowTitlebar(bool),
    SetPosition(Point),
    SetSize(Size),
    SetResizable(bool),
    SetWindowState(window::WindowState),
    ReleaseMouseCapture,
}

#[derive(Clone, Debug, Default)]
pub struct WindowHandle {
    state: Weak<WindowState>,
}

impl PartialEq for WindowHandle {
    fn eq(&self, other: &Self) -> bool {
        match (self.state.upgrade(), other.state.upgrade()) {
            (None, None) => true,
            (Some(s), Some(o)) => Rc::ptr_eq(&s, &o),
            (_, _) => false,
        }
    }
}
impl Eq for WindowHandle {}

unsafe impl HasRawWindowHandle for WindowHandle {
    fn raw_window_handle(&self) -> RawWindowHandle {
        let mut handle = Win32WindowHandle::empty();
        if let Some(hwnd) = self.get_hwnd() {
            handle.hwnd = hwnd as *mut core::ffi::c_void;
            handle.hinstance = unsafe {
                winapi::um::libloaderapi::GetModuleHandleW(0 as LPCWSTR) as *mut core::ffi::c_void
            };
        }
        RawWindowHandle::Win32(handle)
    }
}

unsafe impl HasRawDisplayHandle for WindowHandle {
    /// See:
    ///  * <https://github.com/rust-windowing/raw-window-handle/issues/92>
    ///  * <https://github.com/rust-windowing/winit/blob/92fdf5ba85f920262a61cee4590f4a11ad5738d1/src/platform_impl/windows/window.rs#L285>
    fn raw_display_handle(&self) -> RawDisplayHandle {
        RawDisplayHandle::Windows(WindowsDisplayHandle::empty())
    }
}

/// A handle that can get used to schedule an idle handler. Note that
/// this handle is thread safe. If the handle is used after the hwnd
/// has been destroyed, probably not much will go wrong (the DS_RUN_IDLE
/// message may be sent to a stray window).
#[derive(Clone)]
pub struct IdleHandle {
    pub(crate) hwnd: HWND,
    queue: Arc<Mutex<Vec<IdleKind>>>,
}

/// This represents different Idle Callback Mechanism
enum IdleKind {
    Callback(Box<dyn IdleCallback>),
    Token(IdleToken),
}

#[cfg(feature = "accesskit")]
struct AccessKitActionHandler {
    idle_handle: IdleHandle,
}

/// This is the low level window state. All mutable contents are protected
/// by interior mutability, so we can handle reentrant calls.
struct WindowState {
    hwnd: Cell<HWND>,
    scale: Cell<Scale>,
    area: Cell<ScaledArea>,
    invalid: RefCell<Region>,
    has_menu: Cell<bool>,
    wndproc: Box<dyn WndProc>,
    idle_queue: Arc<Mutex<Vec<IdleKind>>>,
    timers: Arc<Mutex<TimerSlots>>,
    deferred_queue: RefCell<Vec<DeferredOp>>,
    has_titlebar: Cell<bool>,
    is_transparent: Cell<bool>,
    // For resizable borders, window can still be resized with code.
    is_resizable: Cell<bool>,
    handle_titlebar: Cell<bool>,
    active_text_input: Cell<Option<TextFieldToken>>,
    // Is the window focusable ("activatable" in Win32 terminology)?
    // False for tooltips, to prevent stealing focus from owner window.
    is_focusable: bool,
    window_level: WindowLevel,
    #[cfg(feature = "accesskit")]
    uia_init_marker: UiaInitMarker, // zero size
    #[cfg(feature = "accesskit")]
    accesskit_adapter: OnceCell<AccessKitAdapter>,
}

impl std::fmt::Debug for WindowState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.write_str("WindowState{\n")?;
        f.write_str(format!("{:p}", self.hwnd.get()).as_str())?;
        f.write_str("}")?;
        Ok(())
    }
}

/// Generic handler trait for the winapi window procedure entry point.
trait WndProc {
    fn connect(&self, handle: &WindowHandle, state: WndState);

    fn cleanup(&self, hwnd: HWND);

    fn window_proc(&self, hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM)
        -> Option<LRESULT>;
}

// State and logic for the winapi window procedure entry point. Note that this level
// implements policies such as the use of Direct2D for painting.
struct MyWndProc {
    app: Application,
    handle: RefCell<WindowHandle>,
    state: RefCell<Option<WndState>>,
}

/// The mutable state of the window.
struct WndState {
    handler: Box<dyn WinHandler>,
    min_size: Option<Size>,
    keyboard_state: KeyboardState,
    // Stores a set of all mouse buttons that are currently holding mouse
    // capture. When the first mouse button is down on our window we enter
    // capture, and we hold it until the last mouse button is up.
    captured_mouse_buttons: MouseButtons,
    // Is this window the topmost window under the mouse cursor
    has_mouse_focus: bool,
    //TODO: track surrogate orphan
    last_click_time: Instant,
    last_click_pos: (i32, i32),
    click_count: u8,
}

#[derive(Clone, PartialEq, Eq)]
pub struct CustomCursor(Arc<HCursor>);

#[derive(PartialEq, Eq)]
struct HCursor(HCURSOR);

impl Drop for HCursor {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.0);
        }
    }
}

/// Message indicating there are idle tasks to run.
const DS_RUN_IDLE: UINT = WM_USER;

/// Message relaying a request to destroy the window.
///
/// Calling `DestroyWindow` from inside the handler is problematic
/// because it will recursively cause a `WM_DESTROY` message to be
/// sent to the window procedure, even while the handler is borrowed.
/// Thus, the message is dropped and the handler doesn't run.
///
/// As a solution, instead of immediately calling `DestroyWindow`, we
/// send this message to request destroying the window, so that at the
/// time it is handled, we can successfully borrow the handler.
pub(crate) const DS_REQUEST_DESTROY: UINT = WM_USER + 1;

/// Extract the buttons that are being held down from wparam in mouse events.
fn get_buttons(wparam: WPARAM) -> MouseButtons {
    let mut buttons = MouseButtons::new();
    if wparam & MK_LBUTTON != 0 {
        buttons.insert(MouseButton::Left);
    }
    if wparam & MK_RBUTTON != 0 {
        buttons.insert(MouseButton::Right);
    }
    if wparam & MK_MBUTTON != 0 {
        buttons.insert(MouseButton::Middle);
    }
    if wparam & MK_XBUTTON1 != 0 {
        buttons.insert(MouseButton::X1);
    }
    if wparam & MK_XBUTTON2 != 0 {
        buttons.insert(MouseButton::X2);
    }
    buttons
}

fn is_point_in_client_rect(hwnd: HWND, x: i32, y: i32) -> bool {
    unsafe {
        let mut client_rect = mem::MaybeUninit::uninit();
        if GetClientRect(hwnd, client_rect.as_mut_ptr()) == FALSE {
            warn!(
                "failed to get client rect: {}",
                Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
            );
            return false;
        }
        let client_rect = client_rect.assume_init();
        let mouse_point = POINT { x, y };
        PtInRect(&client_rect, mouse_point) != FALSE
    }
}

fn set_style(hwnd: HWND, resizable: bool, titlebar: bool) {
    unsafe {
        let mut style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        if style == 0 {
            warn!(
                "failed to get window style: {}",
                Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
            );
            return;
        }

        if !resizable {
            style &= !(WS_THICKFRAME | WS_MAXIMIZEBOX);
        } else {
            style |= WS_THICKFRAME | WS_MAXIMIZEBOX;
        }
        if !titlebar {
            style &= !(WS_SYSMENU | WS_OVERLAPPED);
        } else {
            style |= WS_MINIMIZEBOX | WS_SYSMENU | WS_OVERLAPPED;
        }
        if SetWindowLongPtrW(hwnd, GWL_STYLE, style as _) == 0 {
            warn!(
                "failed to set the window style: {}",
                Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
            );
        }
        if SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_SHOWWINDOW
                | SWP_NOMOVE
                | SWP_NOZORDER
                | SWP_FRAMECHANGED
                | SWP_NOSIZE
                | SWP_NOOWNERZORDER
                | SWP_NOACTIVATE,
        ) == 0
        {
            warn!(
                "failed to update window style: {}",
                Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
            );
        };
    }
}

impl WndState {
    // Renders but does not present.
    fn render(&mut self, invalid: &Region) {
        self.handler.paint(invalid);
    }

    fn enter_mouse_capture(&mut self, hwnd: HWND, button: MouseButton) {
        if self.captured_mouse_buttons.is_empty() {
            unsafe {
                SetCapture(hwnd);
            }
        }
        self.captured_mouse_buttons.insert(button);
    }

    fn exit_mouse_capture(&mut self, button: MouseButton) -> bool {
        self.captured_mouse_buttons.remove(button);
        self.captured_mouse_buttons.is_empty()
    }
}

impl MyWndProc {
    fn with_window_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(Rc<WindowState>) -> R,
    {
        f(self
            .handle
            // There are no mutable borrows to this: we only use a mutable borrow during
            // initialization.
            .borrow()
            .state
            .upgrade()
            .unwrap()) // WindowState drops after WM_NCDESTROY, so it's always here.
    }

    #[track_caller]
    fn with_wnd_state<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut WndState) -> R,
    {
        let ret = if let Ok(mut s) = self.state.try_borrow_mut() {
            (*s).as_mut().map(f)
        } else {
            error!("failed to borrow WndState at {}", Location::caller());
            None
        };
        if ret.is_some() {
            self.handle_deferred_queue();
        }
        ret
    }

    fn scale(&self) -> Scale {
        self.with_window_state(|state| state.scale.get())
    }

    fn set_scale(&self, scale: Scale) {
        self.with_window_state(move |state| state.scale.set(scale))
    }

    /// Takes the invalid region and returns it, replacing it with the empty region.
    fn take_invalid(&self) -> Region {
        self.with_window_state(|state| {
            mem::replace(&mut *state.invalid.borrow_mut(), Region::EMPTY)
        })
    }

    fn invalidate_rect(&self, rect: Rect) {
        self.with_window_state(|state| state.invalid.borrow_mut().add_rect(rect));
    }

    fn set_area(&self, area: ScaledArea) {
        self.with_window_state(move |state| state.area.set(area))
    }

    fn has_menu(&self) -> bool {
        self.with_window_state(|state| state.has_menu.get())
    }

    fn has_titlebar(&self) -> bool {
        self.with_window_state(|state| state.has_titlebar.get())
    }

    fn resizable(&self) -> bool {
        self.with_window_state(|state| state.is_resizable.get())
    }

    fn is_transparent(&self) -> bool {
        self.with_window_state(|state| state.is_transparent.get())
    }

    fn handle_deferred_queue(&self) {
        let q = self.with_window_state(move |state| state.deferred_queue.replace(Vec::new()));
        for op in q {
            self.handle_deferred(op);
        }
    }

    fn handle_deferred(&self, op: DeferredOp) {
        if let Some(hwnd) = self.handle.borrow().get_hwnd() {
            match op {
                DeferredOp::SetSize(size_dp) => unsafe {
                    let size_px = size_dp.to_px(self.scale());
                    if SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        0,
                        0,
                        size_px.width.round() as i32,
                        size_px.height.round() as i32,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
                    ) == 0
                    {
                        warn!(
                            "failed to resize window: {}",
                            Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                        );
                    };
                },
                DeferredOp::SetPosition(pos_dp) => unsafe {
                    let pos_px = pos_dp.to_px(self.scale());
                    if SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        pos_px.x.round() as i32,
                        pos_px.y.round() as i32,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
                    ) == 0
                    {
                        warn!(
                            "failed to move window: {}",
                            Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                        );
                    };
                },
                DeferredOp::ShowTitlebar(titlebar) => {
                    self.with_window_state(|s| s.has_titlebar.set(titlebar));
                    set_style(hwnd, self.resizable(), titlebar);
                }
                DeferredOp::SetResizable(resizable) => {
                    self.with_window_state(|s| s.is_resizable.set(resizable));
                    set_style(hwnd, resizable, self.has_titlebar());
                }
                DeferredOp::SetWindowState(val) => {
                    let show = if self.handle.borrow().is_focusable() {
                        match val {
                            window::WindowState::Maximized => SW_MAXIMIZE,
                            window::WindowState::Minimized => SW_MINIMIZE,
                            window::WindowState::Restored => SW_RESTORE,
                        }
                    } else {
                        SW_SHOWNOACTIVATE
                    };
                    unsafe {
                        ShowWindow(hwnd, show);
                    }
                }
                DeferredOp::SaveAs(options, token) => {
                    let info = unsafe {
                        get_file_dialog_path(hwnd, FileDialogType::Save, options)
                            .ok()
                            .map(|os_str| FileInfo {
                                path: os_str.into(),
                                format: None,
                            })
                    };
                    self.with_wnd_state(|s| s.handler.save_as(token, info));
                }
                DeferredOp::Open(options, token) => {
                    let info = unsafe {
                        get_file_dialog_path(hwnd, FileDialogType::Open, options)
                            .ok()
                            .map(|s| FileInfo {
                                path: s.into(),
                                format: None,
                            })
                    };
                    self.with_wnd_state(|s| s.handler.open_file(token, info));
                }
                DeferredOp::ContextMenu(menu, pos) => {
                    let hmenu = menu.into_hmenu();
                    let pos = pos.to_px(self.scale()).round();
                    unsafe {
                        let mut point = POINT {
                            x: pos.x as i32,
                            y: pos.y as i32,
                        };
                        ClientToScreen(hwnd, &mut point);
                        if TrackPopupMenu(hmenu, TPM_LEFTALIGN, point.x, point.y, 0, hwnd, null())
                            == FALSE
                        {
                            warn!("failed to track popup menu");
                        }
                    }
                }
                DeferredOp::ReleaseMouseCapture => unsafe {
                    if ReleaseCapture() == FALSE {
                        let result = HRESULT_FROM_WIN32(GetLastError());
                        // When result is zero, it appears to just mean that the capture was already released
                        // (which can easily happen since this is deferred).
                        if result != 0 {
                            warn!("failed to release mouse capture: {}", Error::Hr(result));
                        }
                    }
                },
            }
        } else {
            warn!("Could not get HWND");
        }
    }

    fn get_system_metric(&self, metric: c_int) -> i32 {
        unsafe {
            // This is only supported on windows 10.
            if let Some(func) = OPTIONAL_FUNCTIONS.GetSystemMetricsForDpi {
                let dpi = self.scale().x() * SCALE_TARGET_DPI;
                func(metric, dpi as u32)
            }
            // Support for older versions of windows
            else {
                // Note: On Windows 8.1 GetSystemMetrics() is scaled to the DPI the window
                // was created with, and not the current DPI of the window
                GetSystemMetrics(metric)
            }
        }
    }
}

impl WndProc for MyWndProc {
    fn connect(&self, handle: &WindowHandle, state: WndState) {
        *self.handle.borrow_mut() = handle.clone();
        *self.state.borrow_mut() = Some(state);
        self.state
            .borrow_mut()
            .as_mut()
            .unwrap()
            .handler
            .scale(self.scale());
    }

    fn cleanup(&self, hwnd: HWND) {
        self.app.remove_window(hwnd);
    }

    #[allow(clippy::cognitive_complexity)]
    fn window_proc(
        &self,
        hwnd: HWND,
        msg: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        //println!("wndproc msg: {}", msg);
        match msg {
            WM_CREATE => {
                // Only supported on Windows 10, Could remove this as the 8.1 version below also works on 10..
                let scale_factor = if let Some(func) = OPTIONAL_FUNCTIONS.GetDpiForWindow {
                    unsafe { func(hwnd) as f64 / SCALE_TARGET_DPI }
                }
                // Windows 8.1 Support
                else if let Some(func) = OPTIONAL_FUNCTIONS.GetDpiForMonitor {
                    unsafe {
                        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                        let mut dpiX = 0;
                        let mut dpiY = 0;
                        func(monitor, MDT_EFFECTIVE_DPI, &mut dpiX, &mut dpiY);
                        dpiX as f64 / SCALE_TARGET_DPI
                    }
                } else {
                    1.0
                };
                let scale = Scale::new(scale_factor, scale_factor);
                self.set_scale(scale);

                if let Some(state) = self.handle.borrow().state.upgrade() {
                    state.hwnd.set(hwnd);
                }
                if let Some(state) = self.state.borrow_mut().as_mut() {
                    let handle = self.handle.borrow().to_owned();
                    state.handler.connect(&handle.into());
                }
                Some(0)
            }
            WM_ACTIVATE => {
                if LOWORD(wparam as u32) as u32 != 0 {
                    unsafe {
                        if !self.has_titlebar() && !self.is_transparent() {
                            // This makes windows paint the drop-shadow around the window
                            // since we give it a "1 pixel frame" that we paint over anyway.
                            // From my testing top seems to be the best option when it comes to avoiding resize artifacts.
                            let margins = MARGINS {
                                cxLeftWidth: 0,
                                cxRightWidth: 0,
                                cyTopHeight: 1,
                                cyBottomHeight: 0,
                            };
                            DwmExtendFrameIntoClientArea(hwnd, &margins);
                        }
                        if SetWindowPos(
                            hwnd,
                            HWND_TOPMOST,
                            0,
                            0,
                            0,
                            0,
                            SWP_SHOWWINDOW
                                | SWP_NOMOVE
                                | SWP_NOZORDER
                                | SWP_FRAMECHANGED
                                | SWP_NOSIZE
                                | SWP_NOOWNERZORDER
                                | SWP_NOACTIVATE,
                        ) == 0
                        {
                            warn!(
                                "SetWindowPos failed with error: {}",
                                Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                            );
                        };
                    }
                }
                Some(0)
            }
            WM_ERASEBKGND => Some(0),
            WM_SETFOCUS => {
                self.with_wnd_state(|s| s.handler.got_focus());
                Some(0)
            }
            WM_KILLFOCUS => {
                self.with_wnd_state(|s| s.handler.lost_focus());
                Some(0)
            }
            WM_PAINT => unsafe {
                self.with_wnd_state(|s| {
                    // We call prepare_paint before GetUpdateRect, so that anything invalidated during
                    // prepare_paint will be reflected in GetUpdateRect.
                    s.handler.prepare_paint();

                    let mut rect: RECT = mem::zeroed();
                    // TODO: use GetUpdateRgn for more conservative invalidation
                    GetUpdateRect(hwnd, &mut rect, FALSE);
                    ValidateRect(hwnd, null_mut());
                    let rect_dp = util::recti_to_rect(rect).to_dp(self.scale());
                    if rect_dp.area() != 0.0 {
                        self.invalidate_rect(rect_dp);
                    }
                    let invalid = self.take_invalid();
                    if !invalid.rects().is_empty() {
                        s.handler.rebuild_resources();
                        s.render(&invalid);
                    }
                });
                Some(0)
            },
            WM_DPICHANGED => unsafe {
                let x = HIWORD(wparam as u32) as f64 / SCALE_TARGET_DPI;
                let y = LOWORD(wparam as u32) as f64 / SCALE_TARGET_DPI;
                let scale = Scale::new(x, y);
                self.set_scale(scale);
                let rect: *mut RECT = lparam as *mut RECT;
                SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    (*rect).left,
                    (*rect).top,
                    (*rect).right - (*rect).left,
                    (*rect).bottom - (*rect).top,
                    SWP_NOZORDER
                        | SWP_FRAMECHANGED
                        | SWP_DRAWFRAME
                        | SWP_NOOWNERZORDER
                        | SWP_NOACTIVATE,
                );
                Some(0)
            },
            WM_NCCALCSIZE => unsafe {
                if wparam != 0 && !self.has_titlebar() {
                    if let Ok(handle) = self.handle.try_borrow() {
                        if handle.get_window_state() == window::WindowState::Maximized {
                            // When maximized, windows still adds offsets for the frame
                            // so we counteract them here.
                            let s: *mut NCCALCSIZE_PARAMS = lparam as *mut NCCALCSIZE_PARAMS;
                            if let Some(mut s) = s.as_mut() {
                                let border = self.get_system_metric(SM_CXPADDEDBORDER);
                                let frame = self.get_system_metric(SM_CYSIZEFRAME);
                                s.rgrc[0].top += border + frame;
                                s.rgrc[0].right -= border + frame;
                                s.rgrc[0].left += border + frame;
                                s.rgrc[0].bottom -= border + frame;
                            }
                        }
                    }
                    return Some(0);
                }
                None
            },
            WM_NCHITTEST => unsafe {
                let mut hit = DefWindowProcW(hwnd, msg, wparam, lparam);
                if !self.has_titlebar() && self.resizable() {
                    if let Ok(handle) = self.handle.try_borrow() {
                        if handle.get_window_state() != window::WindowState::Maximized {
                            let mut rect = RECT {
                                left: 0,
                                top: 0,
                                right: 0,
                                bottom: 0,
                            };
                            if GetWindowRect(hwnd, &mut rect) == 0 {
                                warn!(
                                    "failed to get window rect: {}",
                                    Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                                );
                            };
                            let y_cord = HIWORD(lparam as u32) as i16 as i32;
                            let x_cord = LOWORD(lparam as u32) as i16 as i32;
                            let HIT_SIZE = self.get_system_metric(SM_CYSIZEFRAME)
                                + self.get_system_metric(SM_CXPADDEDBORDER);

                            if y_cord - rect.top <= HIT_SIZE {
                                if x_cord - rect.left <= HIT_SIZE {
                                    hit = HTTOPLEFT;
                                } else if rect.right - x_cord <= HIT_SIZE {
                                    hit = HTTOPRIGHT;
                                } else {
                                    hit = HTTOP;
                                }
                            } else if rect.bottom - y_cord <= HIT_SIZE {
                                if x_cord - rect.left <= HIT_SIZE {
                                    hit = HTBOTTOMLEFT;
                                } else if rect.right - x_cord <= HIT_SIZE {
                                    hit = HTBOTTOMRIGHT;
                                } else {
                                    hit = HTBOTTOM;
                                }
                            } else if x_cord - rect.left <= HIT_SIZE {
                                hit = HTLEFT;
                            } else if rect.right - x_cord <= HIT_SIZE {
                                hit = HTRIGHT;
                            }
                        }
                    }
                }
                let mouseDown = GetAsyncKeyState(VK_LBUTTON) < 0;
                if self.with_window_state(|state| state.handle_titlebar.get()) && !mouseDown {
                    self.with_window_state(move |state| state.handle_titlebar.set(false));
                };
                if self.with_window_state(|state| state.handle_titlebar.get()) && hit == HTCLIENT {
                    hit = HTCAPTION;
                }
                Some(hit)
            },
            WM_SIZE => {
                let width = LOWORD(lparam as u32) as u32;
                let height = HIWORD(lparam as u32) as u32;
                if width == 0 || height == 0 {
                    return Some(0);
                }
                self.with_wnd_state(|s| {
                    let scale = self.scale();
                    let area = ScaledArea::from_px((width as f64, height as f64), scale);
                    let size_dp = area.size_dp();
                    self.set_area(area);
                    s.handler.size(size_dp);
                    s.render(&size_dp.to_rect().into());
                })
                .map(|_| 0)
            }
            WM_COMMAND => {
                self.with_wnd_state(|s| s.handler.command(LOWORD(wparam as u32) as u32));
                Some(0)
            }
            //TODO: WM_SYSCOMMAND
            WM_CHAR | WM_SYSCHAR | WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP
            | WM_INPUTLANGCHANGE => {
                unsafe {
                    // We must call keyboard::is_last_message outside of the
                    // WndState borrow below, because is_last_message
                    // calls PeekMessageW, which can make reentrant calls
                    // to our window procedure. There is one known real-world
                    // example of this problem: when Narrator is running
                    // and the user presses Alt+Tab, Narrator's keyboard event
                    // preprocessing withholds the key-down event for Alt
                    // until the user presses Tab, so we receive
                    // WM_KILLFOCUS while we're processing WM_KEYDOWN.
                    let is_last = keyboard::is_last_message(hwnd, msg, lparam);
                    let handled = self.with_wnd_state(|s| {
                        if let Some(event) = s
                            .keyboard_state
                            .process_message(msg, wparam, lparam, is_last)
                        {
                            // If the window doesn't have a menu, then we need to suppress ALT/F10.
                            // Otherwise we will stop getting mouse events for no gain.
                            // When we do have a menu, those keys will focus the menu.
                            let handle_menu = !self.has_menu()
                                && (event.key == KbKey::Alt || event.key == KbKey::F10);
                            match event.state {
                                KeyState::Down => {
                                    let keydown_handled = self.with_window_state(|window_state| {
                                        simulate_input(
                                            &mut *s.handler,
                                            window_state.active_text_input.get(),
                                            event,
                                        )
                                    });
                                    if keydown_handled || handle_menu {
                                        return true;
                                    }
                                }
                                KeyState::Up => {
                                    s.handler.key_up(event);
                                    if handle_menu {
                                        return true;
                                    }
                                }
                            }
                        }
                        false
                    });
                    if handled == Some(true) {
                        Some(0)
                    } else {
                        None
                    }
                }
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                // TODO: apply mouse sensitivity based on
                // SPI_GETWHEELSCROLLLINES setting.
                let handled = self.with_wnd_state(|s| {
                    let system_delta = HIWORD(wparam as u32) as i16 as f64;
                    let down_state = LOWORD(wparam as u32) as usize;
                    let mods = s.keyboard_state.get_modifiers();
                    let is_shift = mods.shift();
                    let wheel_delta = match msg {
                        WM_MOUSEWHEEL if is_shift => Vec2::new(-system_delta, 0.),
                        WM_MOUSEWHEEL => Vec2::new(0., -system_delta),
                        WM_MOUSEHWHEEL => Vec2::new(system_delta, 0.),
                        _ => unreachable!(),
                    };

                    let mut p = POINT {
                        x: LOWORD(lparam as u32) as i16 as i32,
                        y: HIWORD(lparam as u32) as i16 as i32,
                    };
                    unsafe {
                        if ScreenToClient(hwnd, &mut p) == FALSE {
                            warn!(
                                "ScreenToClient failed: {}",
                                Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                            );
                            return false;
                        }
                    }

                    let pos = Point::new(p.x as f64, p.y as f64).to_dp(self.scale());
                    let buttons = get_buttons(down_state);
                    let event = MouseEvent {
                        pos,
                        buttons,
                        mods,
                        count: 0,
                        focus: false,
                        button: MouseButton::None,
                        wheel_delta,
                    };
                    s.handler.mouse_wheel(&event);
                    true
                });
                if handled == Some(false) {
                    None
                } else {
                    Some(0)
                }
            }
            WM_MOUSEMOVE => {
                self.with_wnd_state(|s| {
                    let x = LOWORD(lparam as u32) as i16 as i32;
                    let y = HIWORD(lparam as u32) as i16 as i32;

                    // When the mouse first enters the window client rect we need to register for the
                    // WM_MOUSELEAVE event. Note that WM_MOUSEMOVE is also called even when the
                    // window under the cursor changes without moving the mouse, for example when
                    // our window is first opened under the mouse cursor.
                    if !s.has_mouse_focus && is_point_in_client_rect(hwnd, x, y) {
                        let mut desc = TRACKMOUSEEVENT {
                            cbSize: mem::size_of::<TRACKMOUSEEVENT>() as DWORD,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: HOVER_DEFAULT,
                        };
                        unsafe {
                            if TrackMouseEvent(&mut desc) != FALSE {
                                s.has_mouse_focus = true;
                            } else {
                                warn!(
                                    "failed to TrackMouseEvent: {}",
                                    Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                                );
                            }
                        }
                    }

                    let pos = Point::new(x as f64, y as f64).to_dp(self.scale());
                    let mods = s.keyboard_state.get_modifiers();
                    let buttons = get_buttons(wparam);
                    let event = MouseEvent {
                        pos,
                        buttons,
                        mods,
                        count: 0,
                        focus: false,
                        button: MouseButton::None,
                        wheel_delta: Vec2::ZERO,
                    };
                    s.handler.mouse_move(&event);
                });
                Some(0)
            }
            WM_MOUSELEAVE => {
                self.with_wnd_state(|s| {
                    s.has_mouse_focus = false;
                    s.handler.mouse_leave();
                });
                Some(0)
            }
            // Note: we handle the double-click events out of caution here, but we don't expect
            // to actually receive any, because we don't set CS_DBLCLKS on the window class style.
            // And the reason for that is that we want click counts that go above 2, so it just
            // makes a lot more sense to do the click count logic ourselves.
            WM_LBUTTONDBLCLK | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDBLCLK
            | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDBLCLK | WM_MBUTTONDOWN | WM_MBUTTONUP
            | WM_XBUTTONDBLCLK | WM_XBUTTONDOWN | WM_XBUTTONUP => {
                if let Some(button) = match msg {
                    WM_LBUTTONDBLCLK | WM_LBUTTONDOWN | WM_LBUTTONUP => Some(MouseButton::Left),
                    WM_RBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONUP => Some(MouseButton::Right),
                    WM_MBUTTONDBLCLK | WM_MBUTTONDOWN | WM_MBUTTONUP => Some(MouseButton::Middle),
                    WM_XBUTTONDBLCLK | WM_XBUTTONDOWN | WM_XBUTTONUP => {
                        match HIWORD(wparam as u32) {
                            XBUTTON1 => Some(MouseButton::X1),
                            XBUTTON2 => Some(MouseButton::X2),
                            w => {
                                // Should never happen with current Windows
                                warn!("Received an unknown XBUTTON event ({})", w);
                                None
                            }
                        }
                    }
                    _ => unreachable!(),
                } {
                    self.with_wnd_state(|s| {
                        let down = matches!(
                            msg,
                            WM_LBUTTONDOWN
                                | WM_MBUTTONDOWN
                                | WM_RBUTTONDOWN
                                | WM_XBUTTONDOWN
                                | WM_LBUTTONDBLCLK
                                | WM_MBUTTONDBLCLK
                                | WM_RBUTTONDBLCLK
                                | WM_XBUTTONDBLCLK
                        );
                        let x = LOWORD(lparam as u32) as i16 as i32;
                        let y = HIWORD(lparam as u32) as i16 as i32;
                        let pos = Point::new(x as f64, y as f64).to_dp(self.scale());
                        let mods = s.keyboard_state.get_modifiers();
                        let buttons = get_buttons(wparam);
                        let dct = unsafe { GetDoubleClickTime() };
                        let count = if down {
                            // TODO: it may be more precise to use the timestamp from the event.
                            let this_click = Instant::now();
                            let thresh_x = self.get_system_metric(SM_CXDOUBLECLK);
                            let thresh_y = self.get_system_metric(SM_CYDOUBLECLK);
                            let in_box = (x - s.last_click_pos.0).abs() <= thresh_x / 2
                                && (y - s.last_click_pos.1).abs() <= thresh_y / 2;
                            let threshold = Duration::from_millis(dct as u64);
                            if this_click - s.last_click_time >= threshold || !in_box {
                                s.click_count = 0;
                            }
                            s.click_count = s.click_count.saturating_add(1);
                            s.last_click_time = this_click;
                            s.last_click_pos = (x, y);
                            s.click_count
                        } else {
                            0
                        };
                        let event = MouseEvent {
                            pos,
                            buttons,
                            mods,
                            count,
                            focus: false,
                            button,
                            wheel_delta: Vec2::ZERO,
                        };
                        if count > 0 {
                            s.enter_mouse_capture(hwnd, button);
                            s.handler.mouse_down(&event);
                        } else {
                            s.handler.mouse_up(&event);
                            if s.exit_mouse_capture(button) {
                                self.handle.borrow().defer(DeferredOp::ReleaseMouseCapture);
                            }
                        }
                    });
                }

                Some(0)
            }
            WM_CLOSE => self
                .with_wnd_state(|s| s.handler.request_close())
                .map(|_| 0),
            DS_REQUEST_DESTROY => {
                unsafe {
                    DestroyWindow(hwnd);
                }
                Some(0)
            }
            WM_DESTROY => {
                self.with_wnd_state(|s| s.handler.destroy());
                Some(0)
            }
            WM_TIMER => {
                let id = wparam;
                unsafe {
                    KillTimer(hwnd, id);
                }
                let token = TimerToken::from_raw(id as u64);
                self.handle.borrow().free_timer_slot(token);
                self.with_wnd_state(|s| s.handler.timer(token));
                Some(1)
            }
            WM_CAPTURECHANGED => {
                self.with_wnd_state(|s| s.captured_mouse_buttons.clear());
                Some(0)
            }
            WM_GETMINMAXINFO => {
                let min_max_info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                self.with_wnd_state(|s| {
                    if let Some(min_size_dp) = s.min_size {
                        let min_size_px = min_size_dp.to_px(self.scale());
                        min_max_info.ptMinTrackSize.x = min_size_px.width.round() as i32;
                        min_max_info.ptMinTrackSize.y = min_size_px.height.round() as i32;
                    }
                });
                Some(0)
            }
            DS_RUN_IDLE => self
                .with_wnd_state(|s| {
                    let queue = self.handle.borrow().take_idle_queue();
                    for callback in queue {
                        match callback {
                            IdleKind::Callback(it) => it.call(&mut *s.handler),
                            IdleKind::Token(token) => s.handler.idle(token),
                        }
                    }
                })
                .map(|_| 0),
            #[cfg(feature = "accesskit")]
            WM_GETOBJECT => self
                .handle
                .borrow()
                .state
                .upgrade()
                .and_then(|state| {
                    self.with_wnd_state(|s| {
                        let wparam = accesskit_windows::WPARAM(wparam);
                        let lparam = accesskit_windows::LPARAM(lparam);
                        let idle_queue = &state.idle_queue;
                        let uia_init_marker = state.uia_init_marker; // zero size and Copy
                        state
                            .accesskit_adapter
                            .get_or_init(move || {
                                let initial_tree_state = s.handler.accesskit_tree();
                                let idle_handle = IdleHandle {
                                    hwnd,
                                    queue: Arc::clone(idle_queue),
                                };
                                let action_handler =
                                    Box::new(AccessKitActionHandler { idle_handle });
                                let hwnd = accesskit_windows::HWND(hwnd as _);
                                AccessKitAdapter::new(
                                    hwnd,
                                    initial_tree_state,
                                    action_handler,
                                    uia_init_marker,
                                )
                            })
                            .handle_wm_getobject(wparam, lparam)
                    })
                    .flatten()
                })
                .map(|result| result.into().0),
            _ => None,
        }
    }
}

impl WindowBuilder {
    pub fn new(app: Application) -> WindowBuilder {
        WindowBuilder {
            app,
            handler: None,
            title: String::new(),
            menu: None,
            resizable: true,
            show_titlebar: true,
            transparent: false,
            present_strategy: Default::default(),
            size: None,
            min_size: None,
            position: None,
            level: None,
            state: window::WindowState::Restored,
        }
    }

    /// This takes ownership, and is typically used with UiMain
    pub fn set_handler(mut self, handler: Box<dyn WinHandler>) -> Self {
        self.handler = Some(handler);
        self
    }

    pub fn set_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    pub fn set_min_size(mut self, size: Size) -> Self {
        self.min_size = Some(size);
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn show_titlebar(mut self, show_titlebar: bool) -> Self {
        self.show_titlebar = show_titlebar;
        self
    }

    pub fn set_transparent(mut self, transparent: bool) -> Self {
        // Transparency and Flip is only supported on Windows 8 and newer and
        // require DComposition
        if transparent {
            if OPTIONAL_FUNCTIONS.DCompositionCreateDevice.is_some() {
                self.present_strategy = PresentStrategy::Flip;
                self.transparent = true;
            } else {
                warn!("Transparency requires Windows 8 or newer");
            }
        }
        self
    }

    pub fn set_title<S: Into<String>>(mut self, title: S) -> Self {
        self.title = title.into();
        self
    }

    pub fn set_menu(mut self, menu: Menu) -> Self {
        self.menu = Some(menu);
        self
    }

    pub fn set_position(mut self, position: Point) -> Self {
        self.position = Some(position);
        self
    }

    pub fn set_window_state(mut self, state: window::WindowState) -> Self {
        self.state = state;
        self
    }

    pub fn set_level(mut self, level: WindowLevel) -> Self {
        self.level = Some(level);
        self
    }

    pub fn build(self) -> Result<WindowHandle, Error> {
        unsafe {
            let class_name = util::CLASS_NAME.to_wide();
            let wndproc = MyWndProc {
                app: self.app.clone(),
                handle: Default::default(),
                state: RefCell::new(None),
            };

            // TODO: pos_x and pos_y are only scaled for windows with parents. But they need to be
            // scaled for windows without parents too.
            let (mut pos_x, mut pos_y) = match self.position {
                Some(pos) => (pos.x as i32, pos.y as i32),
                None => (CW_USEDEFAULT, CW_USEDEFAULT),
            };
            let scale = Scale::new(1.0, 1.0);

            let mut area = ScaledArea::default();
            let (width, height) = self
                .size
                .map(|size| {
                    area = ScaledArea::from_dp(size, scale);
                    let size_px = area.size_px();
                    (size_px.width as i32, size_px.height as i32)
                })
                .unwrap_or((CW_USEDEFAULT, CW_USEDEFAULT));

            let (hmenu, accels, has_menu) = match self.menu {
                Some(menu) => {
                    let accels = menu.accels();
                    (menu.into_hmenu(), accels, true)
                }
                None => (0 as HMENU, None, false),
            };

            let mut dwStyle = WS_OVERLAPPEDWINDOW;
            let mut dwExStyle: DWORD = 0;
            let mut focusable = true;
            let mut parent_hwnd = None;
            let window_level;
            if let Some(level) = self.level {
                window_level = level.clone();
                match level {
                    WindowLevel::AppWindow => (),
                    WindowLevel::Tooltip(parent_window_handle)
                    | WindowLevel::DropDown(parent_window_handle)
                    | WindowLevel::Modal(parent_window_handle) => {
                        parent_hwnd = parent_window_handle.0.get_hwnd();
                        dwStyle = WS_POPUP;
                        dwExStyle = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
                        focusable = false;
                        if let Some(point_in_window_coord) = self.position {
                            let screen_point = parent_window_handle.get_position()
                                + point_in_window_coord.to_vec2();
                            let scaled_point = WindowBuilder::scale_sub_window_position(
                                screen_point,
                                parent_window_handle.get_scale(),
                            );
                            pos_x = scaled_point.x as i32;
                            pos_y = scaled_point.y as i32;
                        } else {
                            warn!("No position provided for subwindow!");
                        }
                    }
                }
            } else {
                // Default window level
                window_level = WindowLevel::AppWindow;
            }

            let window = WindowState {
                hwnd: Cell::new(0 as HWND),
                scale: Cell::new(scale),
                area: Cell::new(area),
                invalid: RefCell::new(Region::EMPTY),
                has_menu: Cell::new(has_menu),
                wndproc: Box::new(wndproc),
                idle_queue: Default::default(),
                timers: Arc::new(Mutex::new(TimerSlots::new(1))),
                deferred_queue: RefCell::new(Vec::new()),
                has_titlebar: Cell::new(self.show_titlebar),
                is_resizable: Cell::new(self.resizable),
                is_transparent: Cell::new(self.transparent),
                handle_titlebar: Cell::new(false),
                active_text_input: Cell::new(None),
                is_focusable: focusable,
                window_level,
                #[cfg(feature = "accesskit")]
                uia_init_marker: UiaInitMarker::new(),
                #[cfg(feature = "accesskit")]
                accesskit_adapter: OnceCell::new(),
            };
            let win = Rc::new(window);
            let handle = WindowHandle {
                state: Rc::downgrade(&win),
            };

            let state = WndState {
                handler: self.handler.unwrap(),
                min_size: self.min_size,
                keyboard_state: KeyboardState::new(),
                captured_mouse_buttons: MouseButtons::new(),
                has_mouse_focus: false,
                last_click_time: Instant::now(),
                last_click_pos: (0, 0),
                click_count: 0,
            };
            win.wndproc.connect(&handle, state);

            if !self.resizable {
                dwStyle &= !(WS_THICKFRAME | WS_MAXIMIZEBOX);
            }
            if !self.show_titlebar {
                dwStyle &= !(WS_SYSMENU | WS_OVERLAPPED);
            }

            if self.present_strategy == PresentStrategy::Flip {
                dwExStyle |= WS_EX_NOREDIRECTIONBITMAP;
            }

            match self.state {
                window::WindowState::Maximized => dwStyle |= WS_MAXIMIZE,
                window::WindowState::Minimized => dwStyle |= WS_MINIMIZE,
                _ => (),
            };

            let hwnd = create_window(
                dwExStyle,
                class_name.as_ptr(),
                self.title.to_wide().as_ptr(),
                dwStyle,
                pos_x,
                pos_y,
                width,
                height,
                parent_hwnd.unwrap_or(0 as HWND),
                hmenu,
                0 as HINSTANCE,
                win,
            );
            if hwnd.is_null() {
                return Err(Error::NullHwnd);
            }

            if let Some(size_dp) = self.size {
                if let Ok(scale) = handle.get_scale() {
                    let size_px = size_dp.to_px(scale);
                    if SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        0,
                        0,
                        size_px.width.round() as i32,
                        size_px.height.round() as i32,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                    ) == 0
                    {
                        warn!(
                            "failed to resize window: {}",
                            Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                        );
                    };
                }
            }

            // Dark mode support
            // https://docs.microsoft.com/en-us/windows/apps/desktop/modernize/apply-windows-themes
            const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
            let value: BOOL = 1;
            let value_ptr = &value as *const _ as *const c_void;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                value_ptr,
                mem::size_of::<BOOL>() as u32,
            );

            self.app.add_window(hwnd);

            if let Some(accels) = accels {
                register_accel(hwnd, &accels);
            }
            Ok(handle)
        }
    }

    /// When creating a sub-window, we need to scale its position with respect to its parent.
    /// If there is any error while scaling, log it as a warn and show sub-window in top left corner of screen/window.
    fn scale_sub_window_position(
        un_scaled_sub_window_position: Point,
        parent_window_scale: Result<Scale, crate::Error>,
    ) -> Point {
        match parent_window_scale {
            Ok(s) => un_scaled_sub_window_position.to_px(s),
            Err(e) => {
                warn!("Error with scale: {:?}", e);
                Point::new(0., 0.)
            }
        }
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
type WindowLongPtr = winapi::shared::basetsd::LONG_PTR;
#[cfg(target_arch = "x86")]
type WindowLongPtr = LONG;

pub(crate) unsafe extern "system" fn win_proc_dispatch(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        let create_struct = &*(lparam as *const CREATESTRUCTW);
        let wndproc_ptr = create_struct.lpCreateParams;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, wndproc_ptr as WindowLongPtr);
    }
    let window_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowState;
    let result = {
        if window_ptr.is_null() {
            None
        } else {
            (*window_ptr).wndproc.window_proc(hwnd, msg, wparam, lparam)
        }
    };

    if msg == WM_NCDESTROY && !window_ptr.is_null() {
        (*window_ptr).wndproc.cleanup(hwnd);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(Rc::from_raw(window_ptr));
    }

    match result {
        Some(lresult) => lresult,
        None => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Create a window (same parameters as CreateWindowExW) with associated WndProc.
#[allow(clippy::too_many_arguments)]
unsafe fn create_window(
    dwExStyle: DWORD,
    lpClassName: LPCWSTR,
    lpWindowName: LPCWSTR,
    dwStyle: DWORD,
    x: c_int,
    y: c_int,
    nWidth: c_int,
    nHeight: c_int,
    hWndParent: HWND,
    hMenu: HMENU,
    hInstance: HINSTANCE,
    wndproc: Rc<WindowState>,
) -> HWND {
    CreateWindowExW(
        dwExStyle,
        lpClassName,
        lpWindowName,
        dwStyle,
        x,
        y,
        nWidth,
        nHeight,
        hWndParent,
        hMenu,
        hInstance,
        Rc::into_raw(wndproc) as LPVOID,
    )
}

impl Cursor {
    fn get_hcursor(&self) -> HCURSOR {
        #[allow(deprecated)]
        let name = match self {
            Cursor::Arrow => IDC_ARROW,
            Cursor::IBeam => IDC_IBEAM,
            Cursor::Pointer => IDC_HAND,
            Cursor::Crosshair => IDC_CROSS,
            Cursor::OpenHand => {
                warn!("Cursor::OpenHand not available on windows");
                IDC_ARROW
            }
            Cursor::NotAllowed => IDC_NO,
            Cursor::ResizeLeftRight => IDC_SIZEWE,
            Cursor::ResizeUpDown => IDC_SIZENS,
            Cursor::Custom(c) => {
                return (c.0).0;
            }
        };
        unsafe { LoadCursorW(0 as HINSTANCE, name) }
    }
}

impl WindowHandle {
    pub fn show(&self) {
        if let Some(w) = self.state.upgrade() {
            let hwnd = w.hwnd.get();
            let show = if w.is_focusable {
                match self.get_window_state() {
                    window::WindowState::Maximized => SW_MAXIMIZE,
                    window::WindowState::Minimized => SW_MINIMIZE,
                    _ => SW_SHOWNORMAL,
                }
            } else {
                SW_SHOWNOACTIVATE
            };
            unsafe {
                ShowWindow(hwnd, show);
                UpdateWindow(hwnd);
            }
        }
    }

    pub fn close(&self) {
        if let Some(w) = self.state.upgrade() {
            let hwnd = w.hwnd.get();
            unsafe {
                PostMessageW(hwnd, DS_REQUEST_DESTROY, 0, 0);
            }
        }
    }

    /// Bring this window to the front of the window stack and give it focus.
    pub fn bring_to_front_and_focus(&self) {
        //FIXME: implementation goes here
        warn!("bring_to_front_and_focus not yet implemented on windows");
    }

    pub fn request_anim_frame(&self) {
        if let Some(w) = self.state.upgrade() {
            let hwnd = w.hwnd.get();
            unsafe {
                // With the RDW_INTERNALPAINT flag, RedrawWindow causes a WM_PAINT message, but without
                // invalidating anything. We do this because we won't know the final invalidated region
                // until after calling prepare_paint.
                if RedrawWindow(hwnd, null(), null_mut(), RDW_INTERNALPAINT) == 0 {
                    warn!(
                        "RedrawWindow failed: {}",
                        Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                    );
                }
            }
        }
    }

    pub fn invalidate(&self) {
        if let Some(w) = self.state.upgrade() {
            w.invalid
                .borrow_mut()
                .set_rect(w.area.get().size_dp().to_rect());
        }
        self.request_anim_frame();
    }

    pub fn invalidate_rect(&self, rect: Rect) {
        if let Some(w) = self.state.upgrade() {
            let scale = w.scale.get();
            // We need to invalidate an integer number of pixels, but we also want to keep
            // the invalid region in display points, since that's what we need to pass
            // to WinHandler::paint.
            w.invalid
                .borrow_mut()
                .add_rect(rect.to_px(scale).expand().to_dp(scale));
        }
        self.request_anim_frame();
    }

    fn defer(&self, op: DeferredOp) {
        if let Some(w) = self.state.upgrade() {
            w.deferred_queue.borrow_mut().push(op);
        }
    }

    /// Set the title for this menu.
    pub fn set_title(&self, title: &str) {
        if let Some(w) = self.state.upgrade() {
            let hwnd = w.hwnd.get();
            unsafe {
                if SetWindowTextW(hwnd, title.to_wide().as_ptr()) == FALSE {
                    warn!("failed to set window title '{}'", title);
                }
            }
        }
    }

    pub fn show_titlebar(&self, show_titlebar: bool) {
        self.defer(DeferredOp::ShowTitlebar(show_titlebar));
    }

    pub fn set_position(&self, position: Point) {
        self.defer(DeferredOp::SetWindowState(window::WindowState::Restored));
        if let Some(w) = self.state.upgrade() {
            match &w.window_level {
                WindowLevel::Tooltip(parent_window_handle)
                | WindowLevel::DropDown(parent_window_handle)
                | WindowLevel::Modal(parent_window_handle) => {
                    // Has owned window. Convert point from window coords to screen coords.
                    let screen_position = parent_window_handle.get_position() + position.to_vec2();
                    self.defer(DeferredOp::SetPosition(screen_position));
                }
                WindowLevel::AppWindow => {
                    self.defer(DeferredOp::SetPosition(position));
                }
            }
        }
    }

    // Gets the position of the window in virtual screen coordinates
    pub fn get_position(&self) -> Point {
        if let Some(w) = self.state.upgrade() {
            let hwnd = w.hwnd.get();
            unsafe {
                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                if GetWindowRect(hwnd, &mut rect) == 0 {
                    warn!(
                        "failed to get window rect: {}",
                        Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                    );
                };
                return Point::new(rect.left as f64, rect.top as f64)
                    .to_dp(self.get_scale().unwrap());
            }
        }
        Point::new(0.0, 0.0)
    }

    pub fn content_insets(&self) -> Insets {
        if let Some(w) = self.state.upgrade() {
            let hwnd = w.hwnd.get();
            unsafe {
                let mut info: WINDOWINFO = mem::zeroed();
                info.cbSize = mem::size_of::<WINDOWINFO>() as u32;

                if GetWindowInfo(hwnd, &mut info) == 0 {
                    warn!(
                        "failed to get window info: {}",
                        Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                    );
                };

                let window_frame = Rect::from_points(
                    (info.rcWindow.left as f64, info.rcWindow.top as f64),
                    (info.rcWindow.right as f64, info.rcWindow.bottom as f64),
                );
                let content_frame = Rect::from_points(
                    (info.rcClient.left as f64, info.rcClient.top as f64),
                    (info.rcClient.right as f64, info.rcClient.bottom as f64),
                );

                return (window_frame - content_frame).to_dp(w.scale.get());
            }
        }

        Insets::ZERO
    }

    // Sets the size of the window in DP
    pub fn set_size(&self, size: Size) {
        self.defer(DeferredOp::SetSize(size));
    }

    // Gets the size of the window in device points
    pub fn get_size(&self) -> Size {
        if let Some(w) = self.state.upgrade() {
            let hwnd = w.hwnd.get();
            unsafe {
                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                if GetWindowRect(hwnd, &mut rect) == 0 {
                    warn!(
                        "failed to get window rect: {}",
                        Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                    );
                };
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                return Size::new(width as f64, height as f64).to_dp(w.scale.get());
            }
        }
        Size::new(0.0, 0.0)
    }

    pub fn resizable(&self, resizable: bool) {
        self.defer(DeferredOp::SetResizable(resizable));
    }

    // Sets the window state.
    pub fn set_window_state(&self, state: window::WindowState) {
        self.defer(DeferredOp::SetWindowState(state));
    }

    // Gets the window state.
    pub fn get_window_state(&self) -> window::WindowState {
        // We can not store state internally because it could be modified externally.
        if let Some(w) = self.state.upgrade() {
            let hwnd = w.hwnd.get();
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                if style == 0 {
                    warn!(
                        "failed to get window style: {}",
                        Error::Hr(HRESULT_FROM_WIN32(GetLastError()))
                    );
                }
                if (style & WS_MAXIMIZE) != 0 {
                    window::WindowState::Maximized
                } else if (style & WS_MINIMIZE) != 0 {
                    window::WindowState::Minimized
                } else {
                    window::WindowState::Restored
                }
            }
        } else {
            window::WindowState::Restored
        }
    }

    // Allows windows to handle a custom titlebar like it was the default one.
    pub fn handle_titlebar(&self, val: bool) {
        if let Some(w) = self.state.upgrade() {
            w.handle_titlebar.set(val);
        }
    }

    pub fn set_menu(&self, menu: Menu) {
        let accels = menu.accels();
        let hmenu = menu.into_hmenu();
        if let Some(w) = self.state.upgrade() {
            let hwnd = w.hwnd.get();
            unsafe {
                let old_menu = GetMenu(hwnd);
                if SetMenu(hwnd, hmenu) == FALSE {
                    warn!("failed to set window menu");
                } else {
                    w.has_menu.set(true);
                    DestroyMenu(old_menu);
                }
                if let Some(accels) = accels {
                    register_accel(hwnd, &accels);
                }
            }
        }
    }

    pub fn show_context_menu(&self, menu: Menu, pos: Point) {
        self.defer(DeferredOp::ContextMenu(menu, pos));
    }

    pub fn add_text_field(&self) -> TextFieldToken {
        TextFieldToken::next()
    }

    pub fn remove_text_field(&self, token: TextFieldToken) {
        if let Some(state) = self.state.upgrade() {
            if state.active_text_input.get() == Some(token) {
                state.active_text_input.set(None);
            }
        }
    }

    pub fn set_focused_text_field(&self, active_field: Option<TextFieldToken>) {
        if let Some(state) = self.state.upgrade() {
            state.active_text_input.set(active_field);
        }
    }

    pub fn update_text_field(&self, _token: TextFieldToken, _update: Event) {
        // noop until we get a real text input implementation
    }

    /// Request a timer event.
    ///
    /// The return value is an identifier.
    pub fn request_timer(&self, deadline: Instant) -> TimerToken {
        let (id, elapse) = self.get_timer_slot(deadline);
        let id = self
            .get_hwnd()
            // we reuse timer ids; if this is greater than u32::max we have a problem.
            .map(|hwnd| unsafe { SetTimer(hwnd, id.into_raw() as usize, elapse, None) as u64 })
            .unwrap_or(0);
        TimerToken::from_raw(id)
    }

    /// Set the cursor icon.
    pub fn set_cursor(&mut self, cursor: &Cursor) {
        unsafe {
            SetCursor(cursor.get_hcursor());
        }
    }

    pub fn make_cursor(&self, cursor_desc: &CursorDesc) -> Option<Cursor> {
        if let Some(hwnd) = self.get_hwnd() {
            unsafe {
                let hdc = GetDC(hwnd);
                if hdc.is_null() {
                    return None;
                }
                defer!(ReleaseDC(null_mut(), hdc););

                let mask_dc = CreateCompatibleDC(hdc);
                if mask_dc.is_null() {
                    return None;
                }
                defer!(DeleteDC(mask_dc););

                let bmp_dc = CreateCompatibleDC(hdc);
                if bmp_dc.is_null() {
                    return None;
                }
                defer!(DeleteDC(bmp_dc););

                let width = 1; //cursor_desc.image.width();
                let height = 1; //cursor_desc.image.height();
                let mask = CreateCompatibleBitmap(hdc, width as c_int, height as c_int);
                if mask.is_null() {
                    return None;
                }
                defer!(DeleteObject(mask as _););

                let bmp = CreateCompatibleBitmap(hdc, width as c_int, height as c_int);
                if bmp.is_null() {
                    return None;
                }
                defer!(DeleteObject(bmp as _););

                let old_mask = SelectObject(mask_dc, mask as *mut c_void);
                let old_bmp = SelectObject(bmp_dc, bmp as *mut c_void);

                // for (row_idx, row) in cursor_desc.image.pixel_colors().enumerate() {
                //     for (col_idx, p) in row.enumerate() {
                //         let (r, g, b, a) = p.as_rgba8();
                //         // TODO: what's the story on partial transparency? I couldn't find documentation.
                //         let mask_px = RGB(255 - a, 255 - a, 255 - a);
                //         let bmp_px = RGB(r, g, b);
                //         SetPixel(mask_dc, col_idx as i32, row_idx as i32, mask_px);
                //         SetPixel(bmp_dc, col_idx as i32, row_idx as i32, bmp_px);
                //     }
                // }

                SelectObject(mask_dc, old_mask);
                SelectObject(bmp_dc, old_bmp);

                let mut icon_info = ICONINFO {
                    // 0 means it's a cursor, not an icon.
                    fIcon: 0,
                    xHotspot: cursor_desc.hot.x as DWORD,
                    yHotspot: cursor_desc.hot.y as DWORD,
                    hbmMask: mask,
                    hbmColor: bmp,
                };
                let icon = CreateIconIndirect(&mut icon_info);

                Some(Cursor::Custom(CustomCursor(Arc::new(HCursor(icon)))))
            }
        } else {
            None
        }
    }

    pub fn open_file(&mut self, options: FileDialogOptions) -> Option<FileDialogToken> {
        let tok = FileDialogToken::next();
        self.defer(DeferredOp::Open(options, tok));
        Some(tok)
    }

    pub fn save_as(&mut self, options: FileDialogOptions) -> Option<FileDialogToken> {
        let tok = FileDialogToken::next();
        self.defer(DeferredOp::SaveAs(options, tok));
        Some(tok)
    }

    /// Get the raw HWND handle, for uses that are not wrapped in
    /// druid_win_shell.
    pub fn get_hwnd(&self) -> Option<HWND> {
        self.state.upgrade().map(|w| w.hwnd.get())
    }

    /// Check whether the window can receive keyboard focus. This is generally true,
    /// except for special windows like tooltips.
    pub fn is_focusable(&self) -> bool {
        self.state.upgrade().map(|w| w.is_focusable).unwrap_or(true)
    }

    /// Get a handle that can be used to schedule an idle task.
    pub fn get_idle_handle(&self) -> Option<IdleHandle> {
        self.state.upgrade().map(|w| IdleHandle {
            hwnd: w.hwnd.get(),
            queue: w.idle_queue.clone(),
        })
    }

    fn take_idle_queue(&self) -> Vec<IdleKind> {
        if let Some(w) = self.state.upgrade() {
            mem::take(&mut w.idle_queue.lock().unwrap())
        } else {
            Vec::new()
        }
    }

    /// Get the `Scale` of the window.
    pub fn get_scale(&self) -> Result<Scale, ShellError> {
        Ok(self
            .state
            .upgrade()
            .ok_or(ShellError::WindowDropped)?
            .scale
            .get())
    }

    /// Allocate a timer slot.
    ///
    /// Returns an id and an elapsed time in ms
    fn get_timer_slot(&self, deadline: Instant) -> (TimerToken, u32) {
        if let Some(w) = self.state.upgrade() {
            let mut timers = w.timers.lock().unwrap();
            let id = timers.alloc();
            let elapsed = timers.compute_elapsed(deadline);
            (id, elapsed)
        } else {
            (TimerToken::INVALID, 0)
        }
    }

    fn free_timer_slot(&self, token: TimerToken) {
        if let Some(w) = self.state.upgrade() {
            w.timers.lock().unwrap().free(token)
        }
    }

    #[cfg(feature = "accesskit")]
    pub fn update_accesskit_if_active(
        &self,
        update_factory: impl FnOnce() -> accesskit::TreeUpdate,
    ) {
        if let Some(w) = self.state.upgrade() {
            if let Some(adapter) = w.accesskit_adapter.get() {
                let events = adapter.update(update_factory());
                events.raise();
            }
        }
    }
}

// There is a tiny risk of things going wrong when hwnd is sent across threads.
unsafe impl Send for IdleHandle {}
unsafe impl Sync for IdleHandle {}

impl IdleHandle {
    /// Add an idle handler, which is called (once) when the message loop
    /// is empty. The idle handler will be run from the window's wndproc,
    /// which means it won't be scheduled if the window is closed.
    pub fn add_idle_callback<F>(&self, callback: F)
    where
        F: FnOnce(&mut dyn WinHandler) + Send + 'static,
    {
        let mut queue = self.queue.lock().unwrap();
        if queue.is_empty() {
            unsafe {
                PostMessageW(self.hwnd, DS_RUN_IDLE, 0, 0);
            }
        }
        queue.push(IdleKind::Callback(Box::new(callback)));
    }

    pub fn add_idle_token(&self, token: IdleToken) {
        let mut queue = self.queue.lock().unwrap();
        if queue.is_empty() {
            unsafe {
                PostMessageW(self.hwnd, DS_RUN_IDLE, 0, 0);
            }
        }
        queue.push(IdleKind::Token(token));
    }
}

#[cfg(feature = "accesskit")]
impl accesskit::ActionHandler for AccessKitActionHandler {
    fn do_action(&self, request: accesskit::ActionRequest) {
        self.idle_handle.add_idle_callback(move |handler| {
            handler.accesskit_action(request);
        });
    }
}
