# Capture by HWND with frame context

The additive `capture` module is for applications that already selected a window
and need frame observations as well as pixels. It uses the existing Windows
Graphics Capture mechanism and requires Windows 10 1903 or later for
[CreateForWindow](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow).

```rust,no_run
use std::time::Duration;
use wincap::capture::{self, CapturedFrame};
use windows::Win32::Foundation::HWND;

fn capture(hwnd: HWND) -> capture::Result<CapturedFrame> {
    capture::capture_hwnd(hwnd, None, Duration::from_secs(5))
}
```

`capture_hwnd` never searches for a title and never saves a file. An optional
`WindowRect` selects a frame-relative crop with exclusive right/bottom edges.
Without a crop, it returns all valid frame content at native resolution.

The returned `CapturedFrame` contains:

- `image`: RGBA pixels;
- `item_size`: dimensions when the capture item/frame pool was created;
- `content_size`: valid dimensions from the received frame;
- `texture_size`: actual source texture dimensions, which may exceed content;
- `crop`: the frame-relative rectangle used for the image;
- `received_at_utc`: wall-clock observation in the frame callback;
- `system_relative_time_100ns`: WGC frame time in 100 ns TimeSpan units,
  not UTC or raw QPC counter ticks.

The module does not infer client origins, process identity, DPI mappings, or a
metadata format. Callers needing those must measure and validate them around
capture. It rejects item/content size changes, content outside the texture,
invalid crops, unsupported texture layouts, and invalid row strides.

For manual selection, `window::window_handle(title)` matches an exact
Unicode top-level title, including whitespace. No match and duplicate matches
are distinct errors. Resolve once and pass the returned HWND to `capture_hwnd`.
This is the single title-lookup API, also used by `window::window_sc`.
Title lookup does not initialize WinRT; the capture functions initialize it.

## Errors and lifetime

`capture_hwnd` returns `capture::Result<T>` using the separate
`capture::CaptureError` enum. Native failures, invalid/minimized targets, frame
timeout, target closure, and invalid frame data are returned to the caller.

The timeout bounds waiting for a frame, not native driver calls. The callback
uses a bounded channel and does not block or panic if the receiver is gone.
Frames, event subscriptions, sessions, pools, and texture mappings are cleaned
up on success and failure.

Capture runs on a thread compatible with the WinRT multithreaded apartment and
balances its initialization there. A conflicting apartment returns an error;
STA UI callers can use a worker thread. This API does not change DPI awareness.

## Existing API compatibility

`window::window_handle` retains its `error::Result<HWND>` return type and now
uses the exact Unicode implementation. Ambiguous titles are errors rather than
selecting an arbitrary window; empty, NUL-containing, and overlong titles are
rejected. `error::WindowsCaptureError` adds `AmbiguousWindowTitle` and
`InvalidWindowTitle`, so exhaustive matches must account for these variants.
The existing error variants and their formatting remain unchanged.

`window::get_window_rect` still returns `RECT`. `window::window_sc` uses the
shared title lookup and initializes WinRT itself. Its image-saving and capture
path, and `monitor::monitor_sc`, are otherwise unchanged. Frame context and
bounded waiting remain opt-in through `capture_hwnd`.

## Tests

```powershell
cargo test --locked --lib --test legacy_api
# Requires an interactive Windows desktop and capture support:
cargo test --locked --test capture_hwnd -- --ignored --test-threads=1
```

Synthetic tests cover crop/texture validation, row padding and channel order,
arithmetic limits, timeouts, and callback errors. Compatibility tests check the
original function signatures and exhaustive error matching. Native tests own
nonactivating windows and check repeated capture, dimensions/timing, recovery
after an invalid crop, and shared Unicode title lookup/window capture. They send no
global input.
