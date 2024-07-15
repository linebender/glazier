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

//! Wayland errors

use std::fmt;

use smithay_client_toolkit::reexports::{
    calloop,
    client::{globals::BindError, ConnectError},
};

// TODO: Work out error handling
#[derive(Debug)]
pub enum Error {
    Connect(ConnectError),
    Bind(BindError),
    Calloop(calloop::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            Error::Connect(e) => write!(f, "could not connect to the wayland server: {e:}"),
            Error::Bind(e) => write!(f, "could not bind a wayland global: {e:}"),
            Error::Calloop(e) => write!(f, "calloop failed: {e:}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ConnectError> for Error {
    fn from(value: ConnectError) -> Self {
        Self::Connect(value)
    }
}

impl From<BindError> for Error {
    fn from(value: BindError) -> Self {
        Self::Bind(value)
    }
}

impl From<calloop::Error> for Error {
    fn from(value: calloop::Error) -> Self {
        Self::Calloop(value)
    }
}
