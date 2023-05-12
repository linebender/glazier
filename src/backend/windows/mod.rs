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

//! Windows implementation of glazier.

mod accels;
pub mod application;
pub mod clipboard;
// pub mod dcomp;
pub mod dialog;
pub mod error;
mod keyboard;
pub mod menu;
pub mod msgs;
//pub mod paint;
pub mod screen;
mod timers;
pub mod util;
pub mod window;

// https://docs.microsoft.com/en-us/windows/win32/direct2d/render-targets-overview
// ID2D1RenderTarget is the interface. The other resources inherit from it.
//
// A Render Target creates resources for drawing and performs drawing operations.
//
// - ID2D1HwndRenderTarget objects render content to a window.
// - ID2D1DCRenderTarget objects render to a GDI device context.
// - bitmap render target objects render to off-screen bitmap.
// - DXGI render target objects render to  a DXGI surface for use with Direct3D.
//
// https://docs.microsoft.com/en-us/windows/win32/direct2d/devices-and-device-contexts
// A Device Context, ID2D1DeviceContext, is available as of windows 7 platform update. This
// is the the minimum compatibility target for druid. We are not making an effort to do
// RenderTarget only.
//
// Basically, go from HwndRenderTarget or DxgiSurfaceRenderTarget (2d or 3d) to a Device Context.
// Go back up for particular needs.

use std::fmt::{Debug, Display, Formatter};
use winapi::shared::winerror::HRESULT;

// error handling
pub enum Error {
    WinapiError(HRESULT),
}

impl From<HRESULT> for Error {
    fn from(hr: HRESULT) -> Error {
        Error::WinapiError(hr)
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            Error::WinapiError(hr) => write!(f, "hresult {hr:x}"),
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            Error::WinapiError(hr) => write!(f, "hresult {hr:x}"),
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        "winapi error"
    }
}
