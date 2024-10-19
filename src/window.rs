use crate::{create_capture_item, error, init, take_sc, Handle, WindowRect};
use image::DynamicImage;
use std::{ffi::c_void, mem::size_of, ptr::null_mut};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS;
use windows_sys::Win32::UI::*;

use super::ImageMode;

pub fn window_handle(window_title: &str) -> error::Result<HWND> {
    init();
    let window_name: String = String::from(window_title) + "\0";
    unsafe {
        let handle = WindowsAndMessaging::FindWindowA(null_mut(), window_name.as_ptr());
        if handle.is_null() {
            Err(error::WindowsCaptureError::WindowNotFoundErr)
        } else {
            Ok(HWND(handle))
        }
    }
}

pub fn get_window_rect(window_handle: HWND) -> RECT {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    unsafe {
        match DwmGetWindowAttribute(
            window_handle,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        ) {
            Ok(_) => (),
            Err(error) => println!("Failed to get window rect: {:?}", error),
        }
    }

    rect
}

pub fn window_sc(
    window_title: &str,
    rect: Option<&WindowRect>,
    mode: &ImageMode,
) -> error::Result<DynamicImage> {
    let window_handle = window_handle(window_title)?;

    let capture_rect = match rect {
        Some(window_rect) => RECT {
            left: window_rect.left,
            top: window_rect.top,
            right: window_rect.right,
            bottom: window_rect.bottom,
        },
        None => {
            let r = get_window_rect(window_handle);
            RECT {
                left: 0,
                top: 0,
                right: r.right - r.left,
                bottom: r.bottom - r.top,
            }
        }
    };

    let window_capture_item = create_capture_item(Handle::HWND(window_handle)).unwrap();
    take_sc(&window_capture_item, &capture_rect, mode)
}
