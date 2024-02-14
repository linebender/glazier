#![allow(
    unused,
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    unreachable_pub
)]

use nix::libc::FILE;
include!(concat!(env!("OUT_DIR"), "/xkbcommon_sys.rs"));
