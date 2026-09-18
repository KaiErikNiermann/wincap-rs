use super::{take_frame, CaptureError as Error, CapturedFrame, Result};
use crate::{create_capture_item, Handle, WindowRect};
use std::{marker::PhantomData, rc::Rc, time::Duration};
use windows::Win32::{
    Foundation::HWND,
    System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
    UI::WindowsAndMessaging::{IsIconic, IsWindow},
};

fn validate_window(hwnd: HWND) -> Result<()> {
    if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)).as_bool() } {
        return Err(Error::TargetClosed);
    }
    if unsafe { IsIconic(hwnd).as_bool() } {
        return Err(Error::InvalidInput("cannot capture a minimized window"));
    }
    Ok(())
}

/// Capture an HWND without resolving a title. Crop coordinates are frame-relative.
/// The timeout bounds the frame wait, not native driver/encoding operations.
pub fn capture_hwnd(
    hwnd: HWND,
    rect: Option<&WindowRect>,
    timeout: Duration,
) -> Result<CapturedFrame> {
    validate_window(hwnd)?;
    if timeout.is_zero() {
        return Err(Error::InvalidInput("capture timeout must be positive"));
    }
    let _runtime = Runtime::initialize()?;
    let item = create_capture_item(Handle::HWND(hwnd))?;
    take_frame(&item, rect, timeout)
}

// Balance this API's WinRT initialization on the same OS thread, including S_FALSE.
// Legacy initialization remains unchanged in crate::init.
struct Runtime(PhantomData<Rc<()>>);
impl Runtime {
    fn initialize() -> windows::core::Result<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self(PhantomData))
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe { RoUninitialize() }
    }
}
