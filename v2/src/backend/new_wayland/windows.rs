use std::collections::{BTreeMap, HashMap};

use smithay_client_toolkit::{
    reexports::{
        client::{
            globals::BindError,
            protocol::{
                wl_compositor::{self, WlCompositor},
                wl_surface::{self, WlSurface},
            },
            Connection, Dispatch, Proxy, QueueHandle,
        },
        protocols::{
            wp::{
                fractional_scale::v1::client::{
                    wp_fractional_scale_manager_v1::{self, WpFractionalScaleManagerV1},
                    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
                },
                viewporter::client::wp_viewporter::{self, WpViewporter},
            },
            xdg::shell::client::{
                xdg_surface::{self, XdgSurface},
                xdg_toplevel::{self, XdgToplevel},
                xdg_wm_base::{self, XdgWmBase},
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

        Ok(Self {
            compositor,
            xdg,
            fractional_scale,
            viewporter,
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
            resizable,
            show_titlebar, // TODO: Handling titlebars is tricky on wayland, we need to work out the right API
            transparent: _, // Meaningless on wayland?
            id: _,         // Already used
            app_id,
            size,
            min_size,
            level,
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

        surface.commit();

        windows.surface_to_window.insert(surface.clone(), window_id);
        windows.windows.insert(
            window_id,
            PerWindowState {
                surface,
                xdg_surface,
                toplevel,
                _show_titlebar: show_titlebar,
                resizable,
                initial_configure_complete: false,
                requested_scale: ScaleSource::Fallback(1),

                fractional_scale,
                monitors: Vec::new(),
                applied_scale: Scale::default(),
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
    Fractional(i32),
    Buffer(i32),
    Fallback(i32),
}

impl ScaleSource {
    fn equal(&self, other: &Self) -> bool {
        self.normalise() == other.normalise()
    }

    fn normalise(&self) -> i32 {
        match self {
            ScaleSource::Fractional(v) => *v,
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
    resizable: bool,
    applied_scale: Scale,

    // State
    monitors: Vec<MonitorId>,
    initial_configure_complete: bool,
    requested_scale: ScaleSource,
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
        match event {
            wl_surface::Event::Enter { output } => {
                let new_monitor = plat.state.monitors.window_entered_output(&output, data.0);
                if let Some(monitor) = new_monitor {
                    this.monitors.push(monitor);
                    let new_scale = did_fallback_scale_change(this, &mut plat.state.monitors);
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
                    // Keep only the items which aren't this monitor, i.e. remove this item
                    // TODO: swap_remove?
                    // We expect this array to be
                    let existing_len = this.monitors.len();
                    this.monitors.retain(|item| item != &monitor);
                    if this.monitors.len() == existing_len {
                        tracing::warn!(
                            ?output,
                            "Got window surface leave without corresponding enter being recorded"
                        );
                        return;
                    }
                    let new_scale = did_fallback_scale_change(this, &mut plat.state.monitors);
                }

                // TODO: Recalculate scale if we never got preferred scale through another means
            }
            wl_surface::Event::PreferredBufferScale { factor } => todo!(),
            wl_surface::Event::PreferredBufferTransform { transform } => todo!(),

            event => on_unknown_event(proxy, event),
        }
    }
}

fn did_fallback_scale_change(
    this: &mut PerWindowState,
    outputs: &mut super::outputs::Outputs,
) -> Option<Scale> {
    if this.requested_scale.needs_fallback() && this.initial_configure_complete {
        let new_factor = ScaleSource::Fractional(
            outputs.max_fallback_integer_scale(this.monitors.iter().copied()),
        );
        let was_same = new_factor.equal(&this.requested_scale);
        this.requested_scale = new_factor;
        return Some(new_factor.as_scale());
    }
    return None;
}

impl Dispatch<XdgToplevel, SurfaceUserData> for WaylandPlatform {
    fn event(
        plat: &mut Self,
        proxy: &XdgToplevel,
        event: xdg_toplevel::Event,
        data: &SurfaceUserData,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        let Some(this) = plat.state.windows.windows.get_mut(&data.0) else {
            tracing::error!(?event, "Got unexpected event after deleting a window");
            return;
        };
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => todo!(),
            xdg_toplevel::Event::Close => todo!(),
            xdg_toplevel::Event::ConfigureBounds { width, height } => todo!(),
            xdg_toplevel::Event::WmCapabilities { capabilities } => todo!(),
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
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        let Some(this) = plat.state.windows.windows.get_mut(&data.0) else {
            tracing::error!(?event, "Got unexpected event after deleting a window");
            return;
        };
        match event {
            xdg_surface::Event::Configure { serial } => todo!(),
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
        match event {
            wp_fractional_scale_v1::Event::PreferredScale { scale } => todo!(),
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
