#[cfg(not(all(
    any(feature = "x11", feature = "wayland"),
    any(target_os = "freebsd", target_os = "linux", target_os = "openbsd")
)))]
fn main() {}

#[cfg(all(
    any(feature = "x11", feature = "wayland"),
    any(target_os = "freebsd", target_os = "linux", target_os = "openbsd")
))]
fn main() {
    use pkg_config::probe_library;
    use std::env;
    use std::path::PathBuf;

    let xkbcommon = probe_library("xkbcommon").unwrap();

    #[cfg(feature = "x11")]
    probe_library("xkbcommon-x11").unwrap();

    let mut header = "\
#include <xkbcommon/xkbcommon-compose.h>
#include <xkbcommon/xkbcommon-names.h>
#include <xkbcommon/xkbcommon.h>"
        .to_string();

    if cfg!(feature = "x11") {
        header += "
#include <xkbcommon/xkbcommon-x11.h>";
    }

    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header_contents("wrapper.h", &header)
        .clang_args(
            xkbcommon
                .include_paths
                .iter()
                .filter_map(|path| path.to_str().map(|s| format!("-I{s}"))),
        )
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .prepend_enum_name(false)
        .size_t_is_usize(true)
        .allowlist_function("xkb_.*")
        .allowlist_type("xkb_.*")
        .allowlist_var("XKB_.*")
        .allowlist_type("xcb_connection_t")
        // this needs var args
        .blocklist_function("xkb_context_set_log_fn")
        // we use FILE from libc
        .blocklist_type("FILE")
        .blocklist_type("va_list")
        .default_enum_style(bindgen::EnumVariation::NewType { is_bitfield: true })
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/xkbcommon.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("xkbcommon_sys.rs"))
        .expect("Couldn't write bindings!");
}
