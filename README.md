<div align="center">

# glazier

[![Xi Zulip](https://img.shields.io/badge/Xi%20Zulip-%23glazier-blue?logo=Zulip)](https://xi.zulipchat.com/#narrow/stream/351333-glazier)
[![dependency status](https://deps.rs/repo/github/linebender/glazier/status.svg)](https://deps.rs/repo/github/linebender/glazier)
[![Apache 2.0](https://img.shields.io/badge/license-Apache-blue.svg)](#license)
[![Build Status](https://github.com/linebender/glazier/actions/workflows/ci.yml/badge.svg)](https://github.com/linebender/glazier/actions)
<!-- [![Crates.io](https://img.shields.io/crates/v/glazier.svg)](https://crates.io/crates/glazier) -->
<!-- [![Docs](https://docs.rs/glazier/badge.svg)](https://docs.rs/glazier) -->

</div>

Glazier is an operating system integration layer infrastructure layer intended for
high quality GUI toolkits in Rust. It is agnostic to the choice of drawing, so the
client must provide that, but the goal is to abstract over most of the other
integration points with the underlying operating system.

Primary platforms are Windows, macOS, and Linux. Web is a secondary platform.
Other ports may happen if they are contributed.

This library is currently work in progress and should be considered experimental.
Contributions are welcome, see [CONTRIBUTING](./CONTRIBUTING.md) for more details.

## Community

[![Xi Zulip](https://img.shields.io/badge/Xi%20Zulip-%23glazier-blue?logo=Zulip)](https://xi.zulipchat.com/#narrow/stream/351333-glazier)

Discussion of Xilem development happens in the [Xi Zulip](https://xi.zulipchat.com/), specifically the [#glazier stream](https://xi.zulipchat.com/#narrow/stream/351333-glazier). All public content can be read without logging in

## Scope

The following tasks are in scope. Mostly they are implemented, but as always
there is more refinement to be done.

* Window creation, including subwindows (useful for context menus and the like).

* System menus. These are especially important on macOS.

* Keyboard events.

* Input Method Editor. On macOS, correct handling of dead keys is through the
IME, and an application built on Glazier is expected to handle IME.

* Mouse and pointer (touch and pen) events. Mouse events are currently supported, pointers
are a hoped-for feature.

* Cursors (mouse pointer indicator on the screen), including custom images.

* Providing DPI scaling information to the application.

* Clipboard.

* File dialog.

The general philosophy is that a task is in scope if it requires deep integration
with the platform and is not easy to separate out as a separate library that layers
on top of Glazier.

## Hooks provided

Glazier does not provide drawing primitives and is intended to be agnostic to
the drawing infrastructure. It uses [raw-window-handle] to provide an attachment
point for the drawing code, a widely used abstraction in the Rust ecosystem. A
top priority will be integrating with the [wgpu](https://github.com/gfx-rs/wgpu)
ecosystem. In addition, we would gladly accept integration work to make [Piet]
run on top of Glazier, but this is not a core priority.

We hope to integrate with [AccessKit] for accessibility.

While Glazier currently has primitive support for scheduling repaint cycles,
ultimately we would like to support [frame pacing]. Doing the actual decision
of when to repaint is probably out of scope, but providing portable infrastructure
([CVDisplayLink] on macOS, presentation statistics, scheduling based on high resolution
timers) is in scope.

## Out of scope

We have no solution to interfacing with the system compositor. This is necessary
to handle embedded video content properly, and is also a good way to stitch
together other embedded content such as web views.

Like drawing, most font and text issues are out of scope. Localization and
internationalization are expected to be handled by the layer above, though platform
hooks would be in scope (for example, querying the locale preference).

Glazier does not provide a solution for packaging and distribution of applications.
Work on this is needed, and we would gladly cooperate with such efforts.

## Design

The code in Glazier can be divided into roughly two categories: the
platform agnostic code and types, which are exposed directly, and the
platform-specific implementations of these types, which live in per-backend
directories in `src/backend`. The backend-specific code for the current
backend is reexported as `glazier::backend`.

Glazier does not generally expose backend types directly. Instead, we
expose wrapper structs that define the common interface, and then call
corresponding methods on the concrete type for the current backend.

## Unsafe

Interacting with system APIs is inherently unsafe. One of the goals of
Glazier is to handle almost all interaction with these APIs, exposing
a safe interface to the UI toolkit. The exception is drawing, which will
generally require at least some additional unsafe code for integration.

## Similar libraries

* [winit]. This is by far the most commonly used window creation crate. As
discussed in the links below, the scope is defined quite differently. In general,
winit is probably more suitable for games and game-like applications, while Glazier
is intended to provide more of the full desktop GUI experience, including system
menus and support for IME.

* [baseview]. Another window creation abstraction, motivated mostly by the
audio plugin use case where the module is not in control of its own UI runloop.

## Dependencies

Glazier requires a recent rust toolchain to build; it does not (yet) have an
explicit minimum supported rust version, but the latest stable version should
work.

On Linux and BSD, Glazier also requires `pkg-config` and `clang`,
and the development packages of `libxkbcommon` and `libxcb`, to be installed.
Some of the examples require `vulkan-loader`.

Most distributions have `pkg-config` installed by default. To install the remaining packages on Fedora, run
```
sudo dnf install clang libxkbcommon-x11-devel libxcb-devel vulkan-loader-devel
```
To install them on Debian or Ubuntu, run
```
sudo apt-get install clang libxkbcommon-x11-dev pkg-config libvulkan-dev
```

## Further reading

* [Advice for the next dozen Rust GUIs](https://raphlinus.github.io/rust/gui/2022/07/15/next-dozen-guis.html)

* [Rust GUI Infrastructure](http://www.cmyr.net/blog/rust-gui-infra.html)

* [Text Editing Hates You Too](https://lord.io/text-editing-hates-you-too/)

## License

Licensed under the Apache License, Version 2.0
([LICENSE](LICENSE) or <http://www.apache.org/licenses/LICENSE-2.0>)

## Contribution

Contributions are welcome by pull request. The [Rust code of conduct] applies.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
licensed as above, without any additional terms or conditions.

[druid]: https://github.com/linebender/druid
[winit]: https://github.com/rust-windowing/winit
[baseview]: https://github.com/RustAudio/baseview
[raw-window-handle]: https://github.com/rust-windowing/raw-window-handle
[AccessKit]: https://github.com/AccessKit/accesskit
[frame pacing]: https://raphlinus.github.io/ui/graphics/gpu/2021/10/22/swapchain-frame-pacing.html
[CVDisplayLink]: https://developer.apple.com/documentation/corevideo/cvdisplaylink
[Piet]: https://github.com/linebender/piet
[rust code of conduct]: https://www.rust-lang.org/policies/code-of-conduct
