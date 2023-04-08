use std::collections::HashMap;

use anyhow::bail;
use memchr::memmem;
use x11rb::{
    protocol::xinput::{
        self, ConnectionExt as _, DeviceClass, DeviceClassData, DeviceType, EventMask, Fp3232,
        XIDeviceInfo, XIEventMask,
    },
    xcb_ffi::XCBConnection,
};

use super::application::AppAtoms;

#[derive(Clone, Debug, Default)]
pub struct PointersState {
    pub device_infos: HashMap<u16, DeviceInfo>,
}

#[derive(Clone, Debug)]
pub struct ValuatorInfo {
    pub idx: usize,
    pub min: f64,
    pub max: f64,
    pub resolution: u32,
}

impl ValuatorInfo {
    pub fn read(&self, axisvalues: &[Fp3232]) -> Option<f64> {
        axisvalues
            .get(self.idx)
            .map(|x| fixed_to_floating(*x).clamp(self.min, self.max))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeviceKind {
    Pen,
    Eraser,
    Touch,
    Mouse,
}

impl PointersState {
    pub fn device_info(&self, id: u16) -> Option<&DeviceInfo> {
        self.device_infos.get(&id)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PenValuators {
    pub pressure: Option<ValuatorInfo>,
    pub x_tilt: Option<ValuatorInfo>,
    pub y_tilt: Option<ValuatorInfo>,
}

impl PenValuators {
    fn new(classes: &[DeviceClass], atoms: &AppAtoms) -> Self {
        let mut ret = PenValuators::default();
        for cl in classes {
            if let DeviceClassData::Valuator(val) = &cl.data {
                let info = ValuatorInfo {
                    idx: val.number as usize,
                    min: fixed_to_floating(val.min),
                    max: fixed_to_floating(val.max),
                    resolution: val.resolution,
                };
                if val.label == atoms.ABS_PRESSURE && ret.pressure.is_none() {
                    ret.pressure = Some(info);
                } else if val.label == atoms.ABS_XTILT && ret.x_tilt.is_none() {
                    ret.x_tilt = Some(info);
                } else if val.label == atoms.ABS_YTILT && ret.y_tilt.is_none() {
                    ret.y_tilt = Some(info);
                }
            }
        }
        ret
    }

    fn is_empty(&self) -> bool {
        self.pressure.is_none() && self.x_tilt.is_none() && self.y_tilt.is_none()
    }
}

#[derive(Clone)]
pub struct DeviceInfo {
    pub id: u16,
    pub name: Vec<u8>,
    pub device_type: DeviceType,
    pub device_kind: DeviceKind,
    pub valuators: PenValuators,
}

impl std::fmt::Debug for DeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceInfo")
            .field("id", &self.id)
            .field("name", &String::from_utf8_lossy(&self.name))
            .field("device_type", &self.device_type)
            .field("device_kind", &self.device_kind)
            .field("valuators", &self.valuators)
            .finish()
    }
}

fn is_touch_class(cl: &DeviceClass) -> bool {
    if let DeviceClassData::Touch(touch) = cl.data {
        touch.num_touches > 0
    } else {
        false
    }
}

pub fn fixed_to_floating(x: Fp3232) -> f64 {
    x.integral as f64 + (x.frac as f64) / (1u64 << 32) as f64
}

impl DeviceInfo {
    pub(crate) fn new(dev: XIDeviceInfo, atoms: &AppAtoms) -> DeviceInfo {
        let mut ret = DeviceInfo {
            id: dev.deviceid,
            name: dev.name,
            device_type: dev.type_,
            device_kind: DeviceKind::Mouse,
            valuators: PenValuators::new(&dev.classes, atoms),
        };

        ret.detect_device_kind(&dev.classes);
        ret
    }

    // xinput doesn't tell us directly what "kind" a pointer device is, so we need to infer it.
    // We mainly do this by looking at the `DeviceClass`es: if there's a touch-related class,
    // we declare it as a touch device. Otherwise, if it has pressure or tilt classes, we declare
    // that it's a pen or an eraser (distinguishing between the two by looking at the device name).
    // Otherwise, it's a mouse.
    //
    // Gdk is a reasonable reference for more on this (especially `x11/gdkdevicemanager-xi2.c`, since
    // we're only supported xinput 2). It has (at least) two notions of device type. One of them uses constants like
    // `GDK_DEVICE_TOOL_TYPE_PEN`; it's detected using the "Wacom Tool Type" atom, which
    // doesn't do anything on my system (using XWayland). The other one uses constants
    // like `GDK_SOURCE_PEN`, and this seems like the more useful one to imitate.
    //
    // We do things a bit differently from gdk (e.g. they distinguish between touchscreen and touchpad;
    // they rely more on names and less on `DeviceClass`es), but it's still useful as a reference.
    fn detect_device_kind(&mut self, classes: &[DeviceClass]) {
        self.device_kind = if classes.iter().any(is_touch_class) {
            DeviceKind::Touch
        } else if self.valuators.is_empty() {
            DeviceKind::Mouse
        } else if memmem::find(&self.name, b"eraser").is_some() {
            DeviceKind::Eraser
        } else {
            DeviceKind::Pen
        }
    }
}

pub(crate) fn initialize_pointers(
    conn: &XCBConnection,
    atoms: &AppAtoms,
    window: u32,
) -> anyhow::Result<PointersState> {
    let version = conn.xinput_get_extension_version(b"xinput")?.reply()?;
    if (version.server_major, version.server_minor) < (2, 2) {
        // xinput 2.2 added multitouch; xorg has supported it since 2012
        bail!("xinput version {version:?} found, but we require at least 2.2");
    }

    let devices = conn.xinput_xi_query_device(xinput::Device::ALL)?.reply()?;

    let mut device_infos = HashMap::new();

    for dev in devices.infos {
        if dev.type_ == DeviceType::MASTER_POINTER || dev.type_ == DeviceType::SLAVE_POINTER {
            let id = dev.deviceid;
            let info = DeviceInfo::new(dev, atoms);
            tracing::debug!("found pointer device {info:?}");
            device_infos.insert(id, info);
        }
    }
    conn.xinput_xi_select_events(
        window,
        &[EventMask {
            deviceid: xinput::Device::ALL.into(),
            mask: vec![(XIEventMask::DEVICE_CHANGED | XIEventMask::HIERARCHY)],
        }],
    )?
    .check()?;

    Ok(PointersState { device_infos })
}

pub(crate) fn enable_window_pointers(conn: &XCBConnection, window: u32) -> anyhow::Result<()> {
    conn.xinput_xi_select_events(
        window,
        &[EventMask {
            deviceid: xinput::Device::ALL_MASTER.into(),
            mask: vec![
                (XIEventMask::BUTTON_PRESS
                    | XIEventMask::BUTTON_RELEASE
                    | XIEventMask::MOTION
                    | XIEventMask::TOUCH_BEGIN
                    | XIEventMask::TOUCH_UPDATE
                    | XIEventMask::TOUCH_END),
            ],
        }],
    )?
    .check()?;
    Ok(())
}
