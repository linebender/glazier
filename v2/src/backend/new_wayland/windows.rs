use std::collections::{BTreeMap, HashMap};

use smithay_client_toolkit::{
    reexports::{
        client::{
            globals::BindError,
            protocol::{
                wl_callback::{self, WlCallback},
                wl_compositor::{self, WlCompositor},
                wl_surface::{self, WlSurface},
            },
            Connection, Dispatch, Proxy, QueueHandle,
        },
        csd_frame::WindowManagerCapabilities,
        protocols::{
            wp::{
                fractional_scale::v1::client::{
                    wp_fractional_scale_manager_v1::{self, WpFractionalScaleManagerV1},
                    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
                },
                viewporter::client::wp_viewporter::{self, WpViewporter},
            },
            xdg::{
                decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
                shell::client::{
                    xdg_surface::{self, XdgSurface},
                    xdg_toplevel::{self, WmCapabilities, XdgToplevel},
                    xdg_wm_base::{self, XdgWmBase},
                },
            },
        },
    },
    registry::RegistryState,
};

use crate::{
    monitor::MonitorId,
    window::{Scale, WindowDescription, WindowId, WindowLevel},
};

use super::{on_unknown_event, WaylandPlatform, WaylandState};

pub(super) struct Windowing {
    compositor: WlCompositor,
    xdg: XdgWmBase,
    fractional_scale: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    decoration_manager: Option<ZxdgDecorationManagerV1>,

    windows: BTreeMap<WindowId, PerWindowState>,
    surface_to_window: HashMap<WlSurface, WindowId>,
}

impl Windowing {
    pub(super) fn bind(
        registry: &RegistryState,
        qh: &QueueHandle<WaylandPlatform>,
    ) -> Result<Self, BindError> {
        // All compositors we expect to need to support allow at least version 5
        let compositor = registry.bind_one(qh, 5..=6, ())?;
        // Sway is supposedly still on v2?
        let xdg = registry.bind_one(qh, 2..=6, ())?;
        let fractional_scale = registry.bind_one(qh, 1..=1, ()).ok();
        let viewporter = registry.bind_one(qh, 1..=1, ()).ok();
        let decoration_manager = registry.bind_one(qh, 1..=1, ()).ok();

        Ok(Self {
            compositor,
            xdg,
            fractional_scale,
            viewporter,
            decoration_manager,
            surface_to_window: Default::default(),
            windows: Default::default(),
        })
    }
}

struct SurfaceUserData(WindowId);

impl WaylandState {
    pub(crate) fn new_window(&mut self, mut desc: WindowDescription) -> WindowId {
        let window_id = desc.assign_id();
        let WindowDescription {
            title,
            initial_size,
            min_size,
            level,
            // Meaningless on wayland?
            resizable,
            show_titlebar, // TODO: Handling titlebars is tricky on wayland, we need to work out the right API
            transparent: _,
            app_id,
            id: _,
        } = desc;
        if level != WindowLevel::AppWindow {
            tracing::error!("The Wayland backend doesn't yet support {level:?} windows");
        }

        let qh = &self.wayland_queue;
        let windows = &mut self.windows;

        let surface = windows
            .compositor
            .create_surface(qh, SurfaceUserData(window_id));
        let xdg_surface = windows
            .xdg
            .get_xdg_surface(&surface, qh, SurfaceUserData(window_id));
        let toplevel = xdg_surface.get_toplevel(qh, SurfaceUserData(window_id));
        let fractional_scale = windows
            .fractional_scale
            .as_ref()
            .map(|it| it.get_fractional_scale(&surface, qh, SurfaceUserData(window_id)));

        toplevel.set_title(title);
        if let Some(app_id) = app_id {
            toplevel.set_app_id(app_id);
        }
        if let Some(min_size) = min_size {
            if min_size.is_finite() && min_size.width > 0. && min_size.height > 0. {
                toplevel.set_min_size(min_size.width as i32, min_size.height as i32)
            } else {
                todo!("Couldn't apply invalid min_size: {min_size:?}");
            }
        }
        // Do the first, empty, commit

        surface.commit();

        windows.surface_to_window.insert(surface.clone(), window_id);
        windows.windows.insert(
            window_id,
            PerWindowState {
                toplevel,
                xdg_surface,
                fractional_scale,
                surface,
                _show_titlebar: show_titlebar,

                resizable,
                applied_scale: Scale::default(),
                app_requested_scale: None,

                monitors: Vec::new(),
                initial_configure_complete: false,
                platform_requested_scale: None,
                is_closing: false,

                pending_frame_callback: false,
                will_repaint: false,
            },
        );
        window_id
    }
}

const FRACTIONAL_DENOMINATOR: i32 = 120;

#[derive(Copy, Clone, Debug)]
enum ScaleSource {
    /// Stored as a multiple of 120ths of the 'actual' scale.
    ///
    /// This avoids doing floating point comparisons
    Fractional(u32),
    Buffer(i32),
    Fallback(i32),
}

impl ScaleSource {
    fn equal(&self, other: &Self) -> bool {
        self.normalise() == other.normalise()
    }

    fn normalise(&self) -> i32 {
        match self {
            ScaleSource::Fractional(v) => (*v)
                .try_into()
                .expect("Fractional scale should be sensible"),
            ScaleSource::Buffer(s) | ScaleSource::Fallback(s) => s * FRACTIONAL_DENOMINATOR,
        }
    }

    fn as_scale(&self) -> Scale {
        let factor = match *self {
            ScaleSource::Fractional(num) => (num as f64) / (FRACTIONAL_DENOMINATOR as f64),
            ScaleSource::Buffer(s) => s as f64,
            ScaleSource::Fallback(s) => s as f64,
        };
        Scale::new(factor, factor)
    }

    fn needs_fallback(&self) -> bool {
        match self {
            ScaleSource::Fractional(_) | ScaleSource::Buffer(_) => false,
            ScaleSource::Fallback(_) => true,
        }
    }

    fn better(old: &Self, new: &Self) -> Self {
        match (old, new) {
            (_, new @ ScaleSource::Fractional(_)) => *new,
            (old @ ScaleSource::Fractional(_), _) => *old,
            (_, new @ ScaleSource::Buffer(_)) => *new,
            (old @ ScaleSource::Buffer(_), _) => *old,
            (ScaleSource::Fallback(_), ScaleSource::Fallback(_)) => {
                unreachable!()
            }
        }
    }
}

struct PerWindowState {
    // Wayland properties
    // Dropped before `xdg_surface`
    toplevel: XdgToplevel,
    // Dropped before `surface`
    xdg_surface: XdgSurface,
    // Dropped before `surface`
    fractional_scale: Option<WpFractionalScaleV1>,

    surface: WlSurface,

    // Configuration
    _show_titlebar: bool,
    /// Whether the window should be resizable.
    /// This has a few consequences
    resizable: bool,
    /// The currently active `Scale`
    applied_scale: Scale,
    /// The scale requested by the app. Used to
    app_requested_scale: Option<Scale>,

    // # State
    /// The monitors this window is located within
    monitors: Vec<MonitorId>,

    // Drawing
    /// Whether a `frame` callback is currently active
    ///
    /// ## Context
    /// Wayland requires frame (repainting) callbacks be requested *before* running commit.
    /// However, user code controls when commit is called (generally through calling
    /// wgpu's `present`). Generally, this would mean we would need to know whether the hint
    /// needed to be requested *before* drawing the previous frame, which isn't ideal.
    /// Instead, we follow this procedure:
    /// - Always request a throttling hint before `paint`ing
    /// - Only `paint` in response to that hint *if* the app requested to be redrawn
    /// - `paint` in response to an app request to redraw *only* if there is no running hint
    pending_frame_callback: bool,
    /// Whether an (app launched) repaint request will be sent when the latest
    will_repaint: bool,
    /// We can't draw until the initial configure is complete
    initial_configure_complete: bool,

    platform_requested_scale: Option<ScaleSource>,
    is_closing: bool,
}

/// The context do_paint is called in
enum PaintContext {
    /// Painting occurs during a `frame` callback and finished, we know that there are no more frame callbacks
    Frame,
    /// We're actioning a repaint request, when there was a callback
    Requested,
    /// We're painting in response to a configure event
    Configure,
}

impl WaylandPlatform {
    /// Request that the application paint the window
    fn do_paint(&mut self, win: WindowId, context: PaintContext, force: bool) {
        let this = self
            .state
            .windows
            .windows
            .get_mut(&win)
            .expect("Window present in do_paint");
        if matches!(context, PaintContext::Frame) {
            this.pending_frame_callback = false;
        }
        if matches!(context, PaintContext::Requested) && this.pending_frame_callback && !force {
            // We'll handle this in the frame callback, when that occurs.
            // This ensures throttling is respected
            // This also prevents a hang on startup, although the reason for that occuring isn't clear
            return;
        }
        if !this.initial_configure_complete || (!this.will_repaint && !force) {
            return;
        }
        this.will_repaint = false;
        // If there is not a frame callback in flight, we request it here
        // This branch could be skipped e.g. on `configure`, which ignores frame throttling hints and
        // always paints eagerly, even if there is a frame callback running
        // TODO: Is that the semantics we want?
        if !this.pending_frame_callback {
            this.pending_frame_callback = true;
            this.surface
                .frame(&self.state.wayland_queue, FrameCallbackData(win));
        }
    }
}

impl WaylandState {
    pub(crate) fn set_window_scale(&mut self, win: WindowId, scale: Scale) {
        let Some(this) = self.windows.windows.get_mut(&win) else {
            tracing::error!("Called `set_window_scale` on an unknown/deleted window {win:?}");
            return;
        };
        this.app_requested_scale = Some(scale);
        // TODO: Request repaint?
    }
}

impl Dispatch<WlSurface, SurfaceUserData> for WaylandPlatform {
    fn event(
        plat: &mut Self,
        proxy: &WlSurface,
        event: wl_surface::Event,
        data: &SurfaceUserData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(this) = plat.state.windows.windows.get_mut(&data.0) else {
            tracing::error!(?event, "Got unexpected event after deleting a window");
            return;
        };
        if this.is_closing {
            return;
        }
        match event {
            wl_surface::Event::Enter { output } => {
                let new_monitor = plat.state.monitors.window_entered_output(&output, data.0);
                if let Some(monitor) = new_monitor {
                    this.monitors.push(monitor);
                    let new_scale = did_fallback_scale_change(this, &mut plat.state.monitors);
                    if let Some(new_scale) = new_scale {
                        plat.with_glz(|handler, glz| {
                            handler.platform_proposed_scale(glz, data.0, new_scale);
                        });
                    }
                } else {
                    tracing::warn!(
                        ?output,
                        "Got window surface leave with previously unknown output"
                    );
                }
            }
            wl_surface::Event::Leave { output } => {
                let removed_monitor = plat.state.monitors.window_left_output(&output, data.0);
                if let Some(monitor) = removed_monitor {
                    let existing_len = this.monitors.len();
                    // Keep only the items which aren't this monitor, i.e. remove this item
                    this.monitors.retain(|item| item != &monitor);
                    if this.monitors.len() == existing_len {
                        tracing::warn!(
                            ?output,
                            "Got window surface leave without corresponding enter being recorded"
                        );
                        return;
                    }
                    let new_scale = did_fallback_scale_change(this, &mut plat.state.monitors);
                    if let Some(new_scale) = new_scale {
                        plat.with_glz(|handler, glz| {
                            handler.platform_proposed_scale(glz, data.0, new_scale);
                        });
                    }
                }
            }
            wl_surface::Event::PreferredBufferScale { factor } => {
                let source = ScaleSource::Buffer(factor);
                let proposed_scale = if let Some(existing) = this.platform_requested_scale {
                    let new = ScaleSource::better(&existing, &source);
                    let had_same_value = existing.equal(&new);
                    if had_same_value {
                        return;
                    }
                    new
                } else {
                    *this.platform_requested_scale.insert(source)
                };
                let proposed_scale = proposed_scale.as_scale();

                plat.with_glz(|handler, glz| {
                    handler.platform_proposed_scale(glz, data.0, proposed_scale);
                });
            }
            wl_surface::Event::PreferredBufferTransform { transform } => {
                // TODO: Do we want to abstract over this?
                tracing::info!("Platform suggested a transform {transform:?}");
            }
            event => on_unknown_event(proxy, event),
        }
    }
}

fn did_fallback_scale_change(
    this: &mut PerWindowState,
    outputs: &mut super::outputs::Outputs,
) -> Option<Scale> {
    // If we don't have an existing, that means we didn't request the fallback yet
    if let Some(existing) = this.platform_requested_scale {
        if existing.needs_fallback() {
            let new_factor = ScaleSource::Fallback(
                outputs.max_fallback_integer_scale(this.monitors.iter().copied()),
            );
            let was_same = new_factor.equal(&existing);
            this.platform_requested_scale = Some(new_factor);
            if !was_same {
                return Some(new_factor.as_scale());
            }
        }
    }

    return None;
}

impl Dispatch<XdgToplevel, SurfaceUserData> for WaylandPlatform {
    fn event(
        plat: &mut Self,
        proxy: &XdgToplevel,
        event: xdg_toplevel::Event,
        data: &SurfaceUserData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(this) = plat.state.windows.windows.get_mut(&data.0) else {
            tracing::error!(?event, "Got unexpected event after deleting a window");
            return;
        };
        if this.is_closing {
            return;
        }
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {

                // Test
            }
            xdg_toplevel::Event::Close => {}
            xdg_toplevel::Event::ConfigureBounds { width, height } => {}
            xdg_toplevel::Event::WmCapabilities { capabilities } => {
                // Adapted from Smithay Client Toolkit
                let capabilities = capabilities
                    .chunks_exact(4)
                    .flat_map(TryInto::<[u8; 4]>::try_into)
                    .map(u32::from_ne_bytes)
                    .map(|val| WmCapabilities::try_from(val).map_err(|()| val))
                    .fold(WindowManagerCapabilities::empty(), |acc, capability| {
                        acc | match capability {
                            Ok(WmCapabilities::WindowMenu) => {
                                WindowManagerCapabilities::WINDOW_MENU
                            }
                            Ok(WmCapabilities::Maximize) => WindowManagerCapabilities::MAXIMIZE,
                            Ok(WmCapabilities::Fullscreen) => WindowManagerCapabilities::FULLSCREEN,
                            Ok(WmCapabilities::Minimize) => WindowManagerCapabilities::MINIMIZE,
                            Ok(_) => return acc,
                            Err(v) => {
                                tracing::warn!(?proxy, "Unrecognised window capability {v}");
                                return acc;
                            }
                        }
                    });
            }
            event => on_unknown_event(proxy, event),
        }
    }
}

impl Dispatch<XdgSurface, SurfaceUserData> for WaylandPlatform {
    fn event(
        plat: &mut Self,
        proxy: &XdgSurface,
        event: xdg_surface::Event,
        data: &SurfaceUserData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(this) = plat.state.windows.windows.get_mut(&data.0) else {
            tracing::error!(?event, "Got unexpected event after deleting a window");
            return;
        };
        if this.is_closing {
            return;
        }
        match event {
            xdg_surface::Event::Configure { serial } => {
                if !this.initial_configure_complete {
                    // TODO: What does this mean?
                    this.initial_configure_complete = true;
                }
                if this.platform_requested_scale.is_none() {
                    // We need to use the fallback, so do that
                    let new_factor = ScaleSource::Fallback(
                        plat.state
                            .monitors
                            .max_fallback_integer_scale(this.monitors.iter().copied()),
                    );
                    this.platform_requested_scale = Some(new_factor);
                    plat.with_glz(|handler, glz| {
                        handler.platform_proposed_scale(glz, data.0, new_factor.as_scale())
                    });
                }
                let this = plat
                    .state
                    .windows
                    .windows
                    .get_mut(&data.0)
                    .expect("User's handler can't delete window");
                if this.is_closing {
                    return;
                }
                this.xdg_surface.ack_configure(serial);
            }
            event => on_unknown_event(proxy, event),
        }
    }
}

impl Dispatch<WpFractionalScaleV1, SurfaceUserData> for WaylandPlatform {
    fn event(
        plat: &mut Self,
        proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        data: &SurfaceUserData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(this) = plat.state.windows.windows.get_mut(&data.0) else {
            tracing::error!(?event, "Got unexpected event after deleting a window");
            return;
        };
        if this.is_closing {
            return;
        }
        match event {
            wp_fractional_scale_v1::Event::PreferredScale { scale } => {
                let source = ScaleSource::Fractional(scale);
                let proposed_scale = if let Some(existing) = this.platform_requested_scale {
                    let new = ScaleSource::better(&existing, &source);
                    let had_same_value = existing.equal(&new);
                    if had_same_value {
                        return;
                    }
                    new
                } else {
                    *this.platform_requested_scale.insert(source)
                };
                let proposed_scale = proposed_scale.as_scale();

                plat.with_glz(|handler, glz| {
                    handler.platform_proposed_scale(glz, data.0, proposed_scale);
                });
            }
            event => on_unknown_event(proxy, event),
        }
    }
}

struct FrameCallbackData(WindowId);

impl Dispatch<WlCallback, FrameCallbackData> for WaylandPlatform {
    fn event(
        state: &mut Self,
        proxy: &WlCallback,
        event: wl_callback::Event,
        data: &FrameCallbackData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_callback::Event::Done {
                callback_data: _current_time,
            } => {
                state.do_paint(data.0, PaintContext::Frame, false);
            }
            event => on_unknown_event(proxy, event),
        }
    }
}

// Simple but necessary implementations
impl Dispatch<XdgWmBase, ()> for WaylandPlatform {
    fn event(
        _: &mut Self,
        proxy: &XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_wm_base::Event::Ping { serial } => proxy.pong(serial),
            event => on_unknown_event(proxy, event),
        }
    }
}

// No-op implementations
impl Dispatch<WlCompositor, ()> for WaylandPlatform {
    fn event(
        _: &mut Self,
        proxy: &WlCompositor,
        event: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            event => on_unknown_event(proxy, event),
        }
    }
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for WaylandPlatform {
    fn event(
        _: &mut Self,
        proxy: &WpFractionalScaleManagerV1,
        event: wp_fractional_scale_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            event => on_unknown_event(proxy, event),
        }
    }
}

impl Dispatch<WpViewporter, ()> for WaylandPlatform {
    fn event(
        _: &mut Self,
        proxy: &WpViewporter,
        event: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            event => on_unknown_event(proxy, event),
        }
    }
}

impl Dispatch<ZxdgDecorationManagerV1, ()> for WaylandPlatform {
    fn event(
        _: &mut Self,
        proxy: &ZxdgDecorationManagerV1,
        event: <ZxdgDecorationManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            _ => on_unknown_event(proxy, event),
        }
    }
}
