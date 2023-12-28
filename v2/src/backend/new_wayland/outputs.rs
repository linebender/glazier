use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ops::RangeInclusive,
};

use kurbo_0_9::{Point, Size};
use smithay_client_toolkit::{
    output::Mode,
    reexports::{
        client::{
            globals::BindError,
            protocol::wl_output::{self, Event as WlOutputEvent, Subpixel, Transform, WlOutput},
            Dispatch, Proxy, QueueHandle,
        },
        protocols::xdg::xdg_output::zv1::client::{
            zxdg_output_manager_v1::ZxdgOutputManagerV1,
            zxdg_output_v1::{Event as XdgOutputEvent, ZxdgOutputV1},
        },
    },
    registry::{RegistryHandler, RegistryState},
};
use wayland_backend::protocol::WEnum;

use crate::{monitor::MonitorId, window::WindowId};

use super::WaylandPlatform;

pub(super) struct Outputs {
    xdg_manager: Option<ZxdgOutputManagerV1>,
    outputs: BTreeMap<MonitorId, OutputData>,
    output_to_monitor: HashMap<WlOutput, MonitorId>,
    global_name_to_monitor: BTreeMap<u32, MonitorId>,
}

pub(super) struct OutputInfoForWindow {
    pub monitor: MonitorId,
    /// The integer scale factor of this output
    ///
    /// N.B. Wayland (before wl_compositor v6) forces us to guess which scale
    /// makes the most sense. We choose to provide the *highest* relevant scale,
    /// as there is no further guidance available. I.e., if the window is between
    /// two monitors, one with scale 1, one with scale 2, we give scale 2.
    /// Note that this has a performance cost, but we avoid doing this for good
    /// compositors which *actually tell us what they want*
    ///
    /// In most cases, we will use the fractional scale protocol, which avoids
    /// this concern. Any compositors not implementing that protocol should
    pub scale: i32,
}

impl Outputs {
    pub(super) fn window_entered_output(
        &mut self,
        output: &WlOutput,
        window: WindowId,
    ) -> Option<MonitorId> {
        let Some(monitor) = self.output_to_monitor.get(output).copied() else {
            tracing::warn!("Got window enter for unknown monitor. This is probably because");
            return None;
        };
        let output = self
            .outputs
            .get_mut(&monitor)
            .expect("If we've been added to `output_to_monitor`, we're definitely in `outputs`");
        output.windows.insert(window);
        Some(monitor)
    }

    /// ### Panics
    /// If any of the monitors weren't associated with this Outputs, for some reason
    pub(super) fn max_integer_scale(&mut self, monitors: &[MonitorId]) -> i32 {
        // TODO: Also return the subpixel and transform data (if all the same, give
        // that value, otherwise normal/unknown)
        monitors
            .iter()
            .map(|it| {
                self.outputs.get(it).expect(
                    "Monitor id should have only been available through setting up an output, and correctly removed if the output was deleted",
                )
            })
            .flat_map(|it| &it.info)
            .map(|it| it.scale_factor)
            .reduce(|acc, other| acc.max(other))
            .unwrap_or(1)
    }

    pub(super) fn bind(registry: &RegistryState, qh: &QueueHandle<WaylandPlatform>) -> Outputs {
        let mut ids = Vec::new();

        // All known compositors implement version 4, which moves the `name` from xdg into core wayland
        // For simplicity of implementation, we therefore only support this
        let initial_outputs: Result<Vec<WlOutput>, _> =
            registry.bind_all(qh, XDG_OUTPUT_VERSIONS, |name| {
                let monitor = MonitorId::next();
                ids.push((name, monitor));
                OutputUserData { monitor }
            });
        let initial_outputs = match initial_outputs {
            Ok(it) => it,
            Err(BindError::UnsupportedVersion) => {
                tracing::warn!("Your compositor doesn't support wl_output version 4. Monitor information may not be provided");
                Vec::new()
            }
            Err(BindError::NotPresent) => {
                unreachable!("The behaviour of bind_all has changed to return `NotPresent` when the value is present");
            }
        };

        // We choose to support only version 3, as this is the first version supporting the atomic updates
        // Most compositors we care about implement this, and we don't require this to function
        let xdg_manager: Option<ZxdgOutputManagerV1> =
            match registry.bind_one(qh, 3..=3, OutputManagerData) {
                Ok(it) => Some(it),
                Err(BindError::UnsupportedVersion) => {
                    tracing::warn!("Your compositor does not support XdgOutputManager");
                    None
                }
                Err(BindError::NotPresent) => None,
            };
        let mut outputs = Outputs {
            xdg_manager,
            outputs: BTreeMap::new(),
            output_to_monitor: HashMap::new(),
            global_name_to_monitor: BTreeMap::new(),
        };
        for ((name, monitor), output) in ids.iter().zip(initial_outputs) {
            outputs.setup(qh, *monitor, output, *name);
        }
        outputs
    }
}

/// The (non-deprecated) fields of a wayland - i.e. a display
#[derive(Clone)]
struct OutputInfo {
    subpixel: Subpixel,
    transform: Transform,
    scale_factor: i32,
    mode: Mode,
    logical_position: Point,
    logical_size: Size,
    name: String,
    description: String,
}

struct OutputData {
    /// The name of the global the WlOutput is
    output: WlOutput,
    info: Option<OutputInfo>,
    pending: OutputInfo,
    windows: BTreeSet<WindowId>,
    xdg_output: Option<ZxdgOutputV1>,
}

impl Outputs {
    fn setup(
        &mut self,
        qh: &QueueHandle<WaylandPlatform>,
        monitor: MonitorId,
        output: WlOutput,
        name: u32,
    ) {
        let xdg_output = self
            .xdg_manager
            .as_mut()
            .map(|xdg_manager| xdg_manager.get_xdg_output(&output, qh, OutputUserData { monitor }));
        self.global_name_to_monitor.insert(name, monitor);
        self.output_to_monitor.insert(output.clone(), monitor);
        let output = OutputData {
            output,
            info: None,
            pending: OutputInfo {
                subpixel: Subpixel::Unknown,
                transform: Transform::Normal,
                scale_factor: 1,
                mode: Mode {
                    dimensions: (0, 0),
                    refresh_rate: 0,
                    current: false,
                    preferred: false,
                },
                logical_position: (0., 0.).into(),
                logical_size: (0., 0.).into(),
                name: String::new(),
                description: String::new(),
            },
            windows: BTreeSet::new(),
            xdg_output,
        };
        self.outputs.insert(monitor, output);
    }
}

const XDG_OUTPUT_VERSIONS: RangeInclusive<u32> = 4..=4;

struct OutputUserData {
    monitor: MonitorId,
}

impl Dispatch<WlOutput, OutputUserData> for WaylandPlatform {
    fn event(
        plat: &mut Self,
        _: &WlOutput,
        event: WlOutputEvent,
        data: &OutputUserData,
        _: &smithay_client_toolkit::reexports::client::Connection,
        _: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
    ) {
        let Some(info) = plat.state.monitors.outputs.get_mut(&data.monitor) else {
            tracing::error!("Unknown monitor bound to result");
            return;
        };
        match event {
            WlOutputEvent::Geometry {
                subpixel,
                transform,
                physical_width,
                physical_height,
                x: _x,
                y: _y,
                make: _make,
                model: _model,
            } => {
                match subpixel {
                    WEnum::Value(subpixel) => info.pending.subpixel = subpixel,
                    WEnum::Unknown(e) => {
                        tracing::warn!("Unknown subpixel layout: {e:?}");
                    }
                }
                match transform {
                    WEnum::Value(transform) => info.pending.transform = transform,
                    WEnum::Unknown(e) => {
                        tracing::warn!("Unknown transform: {e:?}");
                    }
                }
            }
            WlOutputEvent::Mode {
                flags,
                width,
                height,
                refresh,
            } => {
                // Mode is *exceedingly* poorly specified. As far as I can tell, this is the best behaviour we can have
                match flags {
                    WEnum::Value(flags) => {
                        let preferred = flags.contains(wl_output::Mode::Preferred);
                        let current = flags.contains(wl_output::Mode::Current);
                        if current {
                            info.pending.mode = Mode {
                                dimensions: (width, height),
                                refresh_rate: refresh,
                                current,
                                preferred,
                            };
                        }
                    }
                    WEnum::Unknown(e) => tracing::info!("Unknown mode flag: {e}"),
                }
            }
            WlOutputEvent::Done => {
                let (scale_factor_changed, new) = match &info.info {
                    None => (true, true),
                    Some(old_info) => (old_info.scale_factor != info.pending.scale_factor, false),
                };
                info.info = Some(info.pending.clone());
                if scale_factor_changed {
                    // TODO: Report an updated scale factor to each associated window
                    for window in &info.windows {}
                }
                if new {
                    // TODO: Report the updated monitor to the handler?
                } else {
                }
            }
            WlOutputEvent::Scale { factor } => info.pending.scale_factor = factor,
            WlOutputEvent::Name { name } => info.pending.name = name,
            WlOutputEvent::Description { description } => info.pending.description = description,
            _ => todo!(),
        }
    }
}

struct OutputManagerData;

impl Dispatch<ZxdgOutputManagerV1, OutputManagerData> for WaylandPlatform {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        event: <ZxdgOutputManagerV1 as Proxy>::Event,
        _: &OutputManagerData,
        _: &smithay_client_toolkit::reexports::client::Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            _ => unreachable!("There are no events for the output manager"),
        }
    }
}

impl Dispatch<ZxdgOutputV1, OutputUserData> for WaylandPlatform {
    fn event(
        plat: &mut Self,
        _: &ZxdgOutputV1,
        event: <ZxdgOutputV1 as smithay_client_toolkit::reexports::client::Proxy>::Event,
        data: &OutputUserData,
        _: &smithay_client_toolkit::reexports::client::Connection,
        _: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
    ) {
        let Some(info) = plat.state.monitors.outputs.get_mut(&data.monitor) else {
            tracing::error!("Unknown monitor bound to result");
            return;
        };
        match event {
            XdgOutputEvent::LogicalPosition { x, y } => {
                info.pending.logical_position = (x as f64, y as f64).into()
            }
            XdgOutputEvent::LogicalSize { width, height } => {
                info.pending.logical_size = (width as f64, height as f64).into()
            }
            //These events are deprecated, so we don't use them
            XdgOutputEvent::Done
            | XdgOutputEvent::Name { .. }
            | XdgOutputEvent::Description { .. } => {}
            _ => todo!(),
        }
    }
}

impl RegistryHandler<WaylandPlatform> for Outputs {
    fn new_global(
        plat: &mut WaylandPlatform,
        _: &smithay_client_toolkit::reexports::client::Connection,
        qh: &QueueHandle<WaylandPlatform>,
        name: u32,
        interface: &str,
        _: u32,
    ) {
        if interface == WlOutput::interface().name {
            let monitor = MonitorId::next();
            let output = match plat.state.registry_state.bind_specific(
                qh,
                name,
                XDG_OUTPUT_VERSIONS,
                OutputUserData { monitor },
            ) {
                Ok(output) => output,
                Err(e) => {
                    tracing::warn!("Couldn't bind new output because:\n\t{e}");
                    return;
                }
            };
            plat.state.monitors.setup(qh, monitor, output, name);
        }
    }

    fn remove_global(
        plat: &mut WaylandPlatform,
        conn: &smithay_client_toolkit::reexports::client::Connection,
        qh: &QueueHandle<WaylandPlatform>,
        name: u32,
        interface: &str,
    ) {
        if interface == WlOutput::interface().name {
            let monitor = plat.state.monitors.global_name_to_monitor.remove(&name);
            if let Some(monitor) = monitor {
                let output = plat.state.monitors.outputs.remove(&monitor).unwrap();
                for window in output.windows {
                    // Notify that they've left the output, i.e. that they should re-calculate their buffer scale
                }
                let _ = plat.state.monitors.output_to_monitor.remove(&output.output);
            }
        }
    }
}
