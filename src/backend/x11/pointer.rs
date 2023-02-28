use std::collections::HashMap;

use anyhow::bail;
use x11rb::{
    protocol::xinput::{self, ConnectionExt as _, EventMask, InputInfo, XIEventMask},
    xcb_ffi::XCBConnection,
};

#[derive(Clone, Debug, Default)]
pub struct PointersState {
    pub device_infos: HashMap<u8, DeviceInfo>,
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub info: xinput::DeviceInfo,
    pub name: Vec<u8>,
    pub inputs: Vec<InputInfo>,
}

pub(crate) fn initialize_pointers(
    conn: &XCBConnection,
    window: u32,
) -> anyhow::Result<PointersState> {
    let version = conn.xinput_get_extension_version(b"xinput")?.reply()?;
    if (version.server_major, version.server_minor) < (2, 2) {
        // xinput 2.2 added multitouch; xorg has supported it since 2012
        bail!("xinput version {version:?} found, but we require at least 2.2");
    }

    let devices = conn.xinput_list_input_devices()?.reply()?;

    let mut device_infos = HashMap::new();
    let mut infos = devices.infos.into_iter();

    for (dev, name) in devices.devices.into_iter().zip(devices.names) {
        tracing::debug!(
            "device {}, named {}, use {:?}",
            dev.device_id,
            String::from_utf8_lossy(&name.name),
            dev.device_use
        );
        device_infos.insert(
            dev.device_id,
            DeviceInfo {
                info: dev,
                name: name.name,
                inputs: (&mut infos).take(dev.num_class_info as usize).collect(),
            },
        );
    }
    conn.xinput_xi_select_events(
        window,
        &[EventMask {
            deviceid: xinput::Device::ALL_MASTER.into(),
            mask: vec![
                (XIEventMask::BUTTON_PRESS | XIEventMask::BUTTON_RELEASE | XIEventMask::MOTION),
            ],
        }],
    )?
    .check()?;

    Ok(PointersState { device_infos })
}
