use image::DynamicImage;
use wincap::{error, monitor, window, ImageMode, WindowRect};
use windows::Win32::Foundation::{HWND, RECT};

#[test]
fn existing_function_signatures_remain_source_compatible() {
    let _: fn(&str) -> error::Result<HWND> = window::window_handle;
    let _: fn(HWND) -> RECT = window::get_window_rect;
    let _: fn(&str, Option<&WindowRect>, &ImageMode) -> error::Result<DynamicImage> =
        window::window_sc;
    let _: fn(Option<&RECT>, &ImageMode) -> error::Result<DynamicImage> = monitor::monitor_sc;
}

#[test]
fn existing_errors_keep_their_format_with_explicit_title_lookup_errors() {
    // Title lookup now reports invalid and ambiguous titles explicitly.
    fn kind(error: &error::WindowsCaptureError) -> u8 {
        match error {
            error::WindowsCaptureError::WindowNotFoundErr => 0,
            error::WindowsCaptureError::DimensionNotFoundErr(_) => 1,
            error::WindowsCaptureError::ImageGenFailedErr(_) => 2,
            error::WindowsCaptureError::ImageSaveFailedErr(_) => 3,
            error::WindowsCaptureError::AmbiguousWindowTitle(_) => 4,
            error::WindowsCaptureError::InvalidWindowTitle(_) => 5,
        }
    }
    let missing = error::WindowsCaptureError::WindowNotFoundErr;
    assert_eq!(kind(&missing), 0);
    assert_eq!(
        error::err_to_string(&missing),
        "Error: Failed to find window handle, is the translator window open?\n",
    );
    assert_eq!(
        kind(&error::WindowsCaptureError::AmbiguousWindowTitle(2)),
        4
    );
    assert_eq!(
        kind(&error::WindowsCaptureError::InvalidWindowTitle("empty")),
        5
    );
}
