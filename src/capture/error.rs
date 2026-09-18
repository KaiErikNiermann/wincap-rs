use std::time::Duration;
use thiserror::Error;

/// Errors for HWND capture. Title lookup uses error::WindowsCaptureError.
#[derive(Error, Debug)]
pub enum CaptureError {
    #[error("Invalid capture argument: {0}")]
    InvalidInput(&'static str),
    #[error("Capture target is no longer available")]
    TargetClosed,
    #[error("Windows Graphics Capture is not supported in this session")]
    Unsupported,
    #[error("No capture frame arrived within {0:?}")]
    CaptureTimeout(Duration),
    #[error("Capture frame channel closed before a frame arrived")]
    FrameChannelClosed,
    #[error("Invalid capture frame: {0}")]
    InvalidFrame(&'static str),
    #[error("Windows capture operation failed")]
    Windows(#[from] windows::core::Error),
}

pub type Result<T> = std::result::Result<T, CaptureError>;
