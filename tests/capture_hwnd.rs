use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::sync_channel,
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};
use wincap::{capture, window, ImageMode, WindowRect};
use windows::{
    core::{w, PCWSTR},
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
};

struct OwnedWindow {
    handle: usize,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl OwnedWindow {
    fn new() -> Self {
        let (sender, receiver) = sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::spawn(move || unsafe {
            let title: Vec<_> = format!("wincap test {} Місто 城", std::process::id())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                360,
                240,
                None,
                None,
                None,
                None,
            )
            .expect("create test window");
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            sender.send(hwnd.0 as usize).unwrap();
            while !thread_stop.load(Ordering::Relaxed) {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                thread::sleep(Duration::from_millis(5));
            }
            DestroyWindow(hwnd).expect("destroy test window");
        });
        let handle = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("window startup");
        Self {
            handle,
            stop,
            thread: Some(thread),
        }
    }
    fn hwnd(&self) -> HWND {
        HWND(self.handle as *mut _)
    }
}
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("window thread");
        }
    }
}

#[test]
#[ignore = "requires an interactive Windows desktop and Windows Graphics Capture"]
fn repeated_frames_preserve_context_and_capture_recovers_after_invalid_crop() {
    let target = OwnedWindow::new();
    let timeout = Duration::from_secs(5);
    assert!(capture::capture_hwnd(HWND::default(), None, timeout).is_err());
    for _ in 0..3 {
        let before = SystemTime::now();
        let frame = capture::capture_hwnd(target.hwnd(), None, timeout).unwrap();
        assert!(frame.received_at_utc >= before && frame.received_at_utc <= SystemTime::now());
        assert!(frame.system_relative_time_100ns > 0);
        assert_eq!(frame.item_size, frame.content_size);
        assert_eq!(frame.image.width(), frame.content_size.width);
        assert_eq!(frame.image.height(), frame.content_size.height);
        assert!(frame.texture_size.width >= frame.content_size.width);
        assert!(frame.texture_size.height >= frame.content_size.height);
        assert_eq!(frame.crop.left, 0);
        assert_eq!(frame.crop.top, 0);
        assert_eq!(frame.crop.right as u32, frame.content_size.width);
        assert_eq!(frame.crop.bottom as u32, frame.content_size.height);
    }
    let invalid = WindowRect {
        left: -1,
        top: 0,
        right: 10,
        bottom: 10,
    };
    assert!(capture::capture_hwnd(target.hwnd(), Some(&invalid), timeout).is_err());
    let crop = WindowRect {
        left: 1,
        top: 2,
        right: 15,
        bottom: 13,
    };
    let frame = capture::capture_hwnd(target.hwnd(), Some(&crop), timeout).unwrap();
    assert_eq!(frame.crop, crop);
    assert_eq!((frame.image.width(), frame.image.height()), (14, 11));
}

#[test]
#[ignore = "requires an interactive Windows desktop and Windows Graphics Capture"]
fn shared_unicode_lookup_and_window_capture_reject_ambiguous_titles() {
    let first = OwnedWindow::new();
    let second = OwnedWindow::new();
    let title = format!("wincap title test {} Місто 城 🦀  ", std::process::id());
    let wide: Vec<_> = title.encode_utf16().chain(Some(0)).collect();
    let longer: Vec<_> = (title.clone() + "longer")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        SetWindowTextW(first.hwnd(), PCWSTR(wide.as_ptr())).unwrap();
        SetWindowTextW(second.hwnd(), PCWSTR(longer.as_ptr())).unwrap();
    }
    // Exact UTF-16 matching preserves trailing spaces and rejects prefixes.
    assert!(matches!(
        window::window_handle(title.trim_end()),
        Err(wincap::error::WindowsCaptureError::WindowNotFoundErr)
    ));
    let hwnd = window::window_handle(&title).unwrap();
    assert_eq!(hwnd, first.hwnd());
    let rect: windows::Win32::Foundation::RECT = window::get_window_rect(hwnd);
    let image = window::window_sc(&title, None, &ImageMode::NoSave).unwrap();
    assert_eq!(image.width(), (rect.right - rect.left) as u32);
    assert_eq!(image.height(), (rect.bottom - rect.top) as u32);

    unsafe {
        SetWindowTextW(second.hwnd(), PCWSTR(wide.as_ptr())).unwrap();
    }
    assert!(matches!(
        window::window_handle(&title),
        Err(wincap::error::WindowsCaptureError::AmbiguousWindowTitle(2))
    ));
    assert!(matches!(
        window::window_sc(&title, None, &ImageMode::NoSave),
        Err(wincap::error::WindowsCaptureError::AmbiguousWindowTitle(2))
    ));
}
