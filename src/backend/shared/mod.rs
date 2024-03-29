// Copyright 2020 The Druid Authors.
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

//! Logic that is shared by more than one backend.

cfg_if::cfg_if! {
    if #[cfg(any(target_os = "freebsd", target_os = "macos", target_os = "linux", target_os = "openbsd"))] {
        mod keyboard;
        pub use keyboard::*;
    }
}
cfg_if::cfg_if! {
    if #[cfg(all(any(target_os = "freebsd", target_os = "linux"), any(feature = "x11", feature = "wayland")))] {
        pub(crate) mod xkb;
        pub(crate) mod linux;
    }
}
cfg_if::cfg_if! {
    if #[cfg(all(any(target_os = "freebsd", target_os = "linux"), any(feature = "x11")))] {
        // TODO: This might also be used in Wayland, but we don't implement timers there yet
        mod timer;
        pub(crate) use timer::*;
    }
}
