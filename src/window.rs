use crate::error::WindowsCaptureError as Error;
use crate::{create_capture_item, error, init, take_sc, Handle, WindowRect};
use image::DynamicImage;
use std::{ffi::c_void, mem::size_of};
use windows::core::BOOL;
use windows::Win32::Foundation::{
    GetLastError, SetLastError, ERROR_INVALID_WINDOW_HANDLE, ERROR_SUCCESS, HWND, LPARAM, RECT,
};
use windows::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS;
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW};

use super::ImageMode;

struct TitleSearch {
    title: Vec<u16>,
    buffer: Vec<u16>,
    first: Option<HWND>,
    count: usize,
    error: Option<windows::core::Error>,
}
unsafe extern "system" fn match_title(hwnd: HWND, parameter: LPARAM) -> BOOL {
    let search = unsafe { &mut *(parameter.0 as *mut TitleSearch) };
    unsafe { SetLastError(ERROR_SUCCESS) };
    // Extra space prevents accepting a truncated prefix of a longer title.
    let count = unsafe { GetWindowTextW(hwnd, &mut search.buffer) };
    if count == 0 {
        let code = unsafe { GetLastError() };
        if code != ERROR_SUCCESS && code != ERROR_INVALID_WINDOW_HANDLE {
            search.error = Some(windows::core::Error::from_hresult(
                windows::core::HRESULT::from_win32(code.0),
            ));
            return false.into();
        }
    } else if count as usize == search.title.len()
        && search.buffer[..count as usize] == search.title
    {
        search.first.get_or_insert(hwnd);
        search.count += 1;
    }
    true.into()
}

/// Resolve one exact Unicode top-level title; reject duplicate matches.
pub fn window_handle(title: &str) -> error::Result<HWND> {
    if title.is_empty() || title.contains('\0') {
        return Err(Error::InvalidWindowTitle(
            "title must be nonempty and contain no NUL",
        ));
    }
    let title: Vec<u16> = title.encode_utf16().collect();
    if title.len() > 32767 {
        return Err(Error::InvalidWindowTitle("window title is too long"));
    }
    let mut search = TitleSearch {
        buffer: vec![0; title.len() + 2],
        title,
        first: None,
        count: 0,
        error: None,
    };
    let enumerated = unsafe {
        EnumWindows(
            Some(match_title),
            LPARAM(&mut search as *mut TitleSearch as isize),
        )
    };
    if let Some(error) = search.error {
        return Err(error.into());
    }
    enumerated?;
    match (search.count, search.first) {
        (0, _) => Err(Error::WindowNotFoundErr),
        (1, Some(hwnd)) => Ok(hwnd),
        (count, _) => Err(Error::AmbiguousWindowTitle(count)),
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
    init();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_titles_before_enumerating_windows() {
        for title in [String::new(), "title\0suffix".into(), "a".repeat(32768)] {
            assert!(matches!(
                window_handle(&title),
                Err(Error::InvalidWindowTitle(_))
            ));
        }
    }
}
