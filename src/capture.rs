//! Opt-in capture with frame context, validation, and a bounded frame wait.
//! Use window::window_handle for shared exact Unicode title selection.

mod error;
mod window;
pub use error::{CaptureError, Result};
pub use window::capture_hwnd;

use crate::{devices, WindowRect};
use image::{DynamicImage, RgbaImage};
use std::{
    sync::mpsc::{sync_channel, Receiver, RecvTimeoutError},
    time::{Duration, SystemTime},
};
use windows::{
    core::{IInspectable, Interface},
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{
            Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
            GraphicsCaptureSession,
        },
        DirectX::DirectXPixelFormat,
        SizeInt32,
    },
    Win32::Graphics::{
        Direct3D11::{
            ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, D3D11_BOX,
            D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_STAGING,
        },
        Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
    },
};
use CaptureError as Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureSize {
    pub width: u32,
    pub height: u32,
}

/// Native-resolution pixels and observations from the same received frame.
/// This contains no guessed client origin or process identity.
#[derive(Debug)]
pub struct CapturedFrame {
    pub image: DynamicImage,
    pub item_size: CaptureSize,
    pub content_size: CaptureSize,
    pub texture_size: CaptureSize,
    pub crop: WindowRect,
    pub received_at_utc: SystemTime,
    /// WGC TimeSpan ticks (100 ns), not raw QPC ticks or UTC.
    pub system_relative_time_100ns: i64,
}

fn size(value: SizeInt32) -> Result<CaptureSize> {
    if value.Width <= 0 || value.Height <= 0 {
        return Err(Error::InvalidFrame("empty or negative content size"));
    }
    Ok(CaptureSize {
        width: value.Width as u32,
        height: value.Height as u32,
    })
}

fn validate_crop(
    item: CaptureSize,
    content: CaptureSize,
    texture: CaptureSize,
    crop: WindowRect,
) -> Result<CaptureSize> {
    if item != content {
        return Err(Error::InvalidFrame(
            "content size changed since capture item creation; retry",
        ));
    }
    if content.width == 0
        || content.height == 0
        || content.width > texture.width
        || content.height > texture.height
    {
        return Err(Error::InvalidFrame(
            "content does not fit the source texture",
        ));
    }
    if crop.left < 0
        || crop.top < 0
        || crop.right <= crop.left
        || crop.bottom <= crop.top
        || crop.right as u32 > content.width
        || crop.bottom as u32 > content.height
    {
        return Err(Error::InvalidFrame(
            "crop is empty or outside valid frame content",
        ));
    }
    Ok(CaptureSize {
        width: (crop.right - crop.left) as u32,
        height: (crop.bottom - crop.top) as u32,
    })
}

struct FrameLease {
    frame: Direct3D11CaptureFrame,
    closed: bool,
}
impl FrameLease {
    fn close(&mut self) -> windows::core::Result<()> {
        self.frame.Close()?;
        self.closed = true;
        Ok(())
    }
}
impl Drop for FrameLease {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.frame.Close();
        }
    }
}

// Partial setup and error returns also detach handlers and close native resources.
struct Session {
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: Option<GraphicsCaptureSession>,
    frame_handler: Option<i64>,
    closed_handler: Option<i64>,
    closed: bool,
}
impl Session {
    fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut first_error = None;
        let mut record = |result: windows::core::Result<()>| {
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        };
        if let Some(token) = self.frame_handler.take() {
            record(self.pool.RemoveFrameArrived(token));
        }
        if let Some(token) = self.closed_handler.take() {
            record(self.item.RemoveClosed(token));
        }
        if let Some(session) = self.session.take() {
            record(session.Close());
        }
        record(self.pool.Close());
        match first_error {
            Some(error) => Err(error.into()),
            None => Ok(()),
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn receive<T>(receiver: &Receiver<Result<T>>, timeout: Duration) -> Result<T> {
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err(Error::CaptureTimeout(timeout)),
        Err(RecvTimeoutError::Disconnected) => Err(Error::FrameChannelClosed),
    }
}

pub(crate) fn take_frame(
    item: &GraphicsCaptureItem,
    crop: Option<&WindowRect>,
    timeout: Duration,
) -> Result<CapturedFrame> {
    if timeout.is_zero() {
        return Err(Error::InvalidInput("capture timeout must be positive"));
    }
    if !GraphicsCaptureSession::IsSupported()? {
        return Err(Error::Unsupported);
    }
    let initial_size = item.Size()?;
    let item_size = size(initial_size)?;
    let device =
        devices::try_create_d3d_device()?.ok_or(Error::InvalidFrame("D3D11 returned no device"))?;
    let context = unsafe { device.GetImmediateContext()? };
    let direct_device = devices::create_direct3d_device(&device)?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &direct_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        initial_size,
    )?;
    let mut resources = Session {
        item: item.clone(),
        pool,
        session: None,
        frame_handler: None,
        closed_handler: None,
        closed: false,
    };
    resources.session = Some(resources.pool.CreateCaptureSession(item)?);
    let (sender, receiver) = sync_channel(1);
    let frame_sender = sender.clone();
    resources.frame_handler = Some(resources.pool.FrameArrived(&TypedEventHandler::<
        Direct3D11CaptureFramePool,
        IInspectable,
    >::new(move |pool, _| {
        let result = match pool.as_ref() {
            Some(pool) => pool
                .TryGetNextFrame()
                .map(|frame| {
                    (
                        FrameLease {
                            frame,
                            closed: false,
                        },
                        SystemTime::now(),
                    )
                })
                .map_err(Error::from),
            None => Err(Error::FrameChannelClosed),
        };
        // Never block or panic on the WGC worker. Unused/late frames close on drop.
        let _ = frame_sender.try_send(result);
        Ok(())
    }))?);
    resources.closed_handler = Some(item.Closed(&TypedEventHandler::<
        GraphicsCaptureItem,
        IInspectable,
    >::new(move |_, _| {
        let _ = sender.try_send(Err(Error::TargetClosed));
        Ok(())
    }))?);
    if let Some(session) = &resources.session {
        session.StartCapture()?;
    }
    let (mut frame, received_at_utc) = receive(&receiver, timeout)?;
    let content_size = size(frame.frame.ContentSize()?)?;
    let system_relative_time_100ns = frame.frame.SystemRelativeTime()?.Duration;
    if system_relative_time_100ns < 0 {
        return Err(Error::InvalidFrame("negative frame timestamp"));
    }
    let crop = crop.copied().unwrap_or(WindowRect {
        left: 0,
        top: 0,
        right: content_size.width as i32,
        bottom: content_size.height as i32,
    });
    let (texture, texture_size, image_size) = copy_texture(
        &frame.frame,
        &device,
        &context,
        item_size,
        content_size,
        crop,
    )?;
    let image = read_texture(&texture, &context, image_size)?;
    frame.close()?;
    resources.close()?;
    Ok(CapturedFrame {
        image,
        item_size,
        content_size,
        texture_size,
        crop,
        received_at_utc,
        system_relative_time_100ns,
    })
}

fn copy_texture(
    frame: &Direct3D11CaptureFrame,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    item: CaptureSize,
    content: CaptureSize,
    crop: WindowRect,
) -> Result<(ID3D11Texture2D, CaptureSize, CaptureSize)> {
    unsafe {
        let source: ID3D11Texture2D = devices::get_d3d_interface_from_object(&frame.Surface()?)?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        source.GetDesc(&mut desc);
        let texture_size = CaptureSize {
            width: desc.Width,
            height: desc.Height,
        };
        let image_size = validate_crop(item, content, texture_size, crop)?;
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM
            || desc.SampleDesc.Count != 1
            || desc.ArraySize != 1
            || desc.MipLevels != 1
        {
            return Err(Error::InvalidFrame("unsupported texture format or layout"));
        }
        desc.Width = image_size.width;
        desc.Height = image_size.height;
        desc.BindFlags = 0;
        desc.MiscFlags = 0;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut texture = None;
        device.CreateTexture2D(&desc, None, Some(&mut texture))?;
        let texture = texture.ok_or(Error::InvalidFrame("D3D11 returned no staging texture"))?;
        context.CopySubresourceRegion(
            Some(&texture.cast()?),
            0,
            0,
            0,
            0,
            Some(&source.cast()?),
            0,
            Some(&D3D11_BOX {
                left: crop.left as u32,
                top: crop.top as u32,
                right: crop.right as u32,
                bottom: crop.bottom as u32,
                front: 0,
                back: 1,
            }),
        );
        Ok((texture, texture_size, image_size))
    }
}

fn row_layout(size: CaptureSize, pitch: usize) -> Result<(usize, usize)> {
    let bytes = (size.width as usize)
        .checked_mul(4)
        .ok_or(Error::InvalidFrame("row size overflow"))?;
    if bytes == 0 || size.height == 0 || pitch < bytes {
        return Err(Error::InvalidFrame("invalid row pitch or image size"));
    }
    let span = (size.height as usize - 1)
        .checked_mul(pitch)
        .and_then(|n| n.checked_add(bytes))
        .filter(|&n| n <= isize::MAX as usize)
        .ok_or(Error::InvalidFrame("mapped size overflow"))?;
    Ok((bytes, span))
}

fn decode_rows(source: &[u8], size: CaptureSize, pitch: usize) -> Result<DynamicImage> {
    let (bytes, span) = row_layout(size, pitch)?;
    if source.len() < span {
        return Err(Error::InvalidFrame(
            "mapped buffer is shorter than its rows",
        ));
    }
    // row_layout proves that packed length is no larger than the checked span.
    let mut output = vec![0; bytes * size.height as usize];
    for row in 0..size.height as usize {
        for (src, dst) in source[row * pitch..row * pitch + bytes]
            .chunks_exact(4)
            .zip(output[row * bytes..(row + 1) * bytes].chunks_exact_mut(4))
        {
            dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
        }
    }
    let image = RgbaImage::from_raw(size.width, size.height, output)
        .ok_or(Error::InvalidFrame("invalid RGBA image layout"))?;
    Ok(DynamicImage::ImageRgba8(image))
}

struct Mapped<'a> {
    context: &'a ID3D11DeviceContext,
    resource: ID3D11Resource,
}
impl Drop for Mapped<'_> {
    fn drop(&mut self) {
        unsafe { self.context.Unmap(Some(&self.resource), 0) }
    }
}
fn read_texture(
    texture: &ID3D11Texture2D,
    context: &ID3D11DeviceContext,
    size: CaptureSize,
) -> Result<DynamicImage> {
    unsafe {
        let resource: ID3D11Resource = texture.cast()?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(Some(&resource), 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let _unmap = Mapped { context, resource };
        let (_, span) = row_layout(size, mapped.RowPitch as usize)?;
        if mapped.pData.is_null() {
            return Err(Error::InvalidFrame("null mapped texture data"));
        }
        let bytes = std::slice::from_raw_parts(mapped.pData as *const u8, span);
        decode_rows(bytes, size, mapped.RowPitch as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FULL: CaptureSize = CaptureSize {
        width: 100,
        height: 80,
    };
    const RECT: WindowRect = WindowRect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 80,
    };
    #[test]
    fn accepts_exact_edges_and_larger_texture() {
        assert_eq!(
            validate_crop(
                FULL,
                FULL,
                CaptureSize {
                    width: 128,
                    height: 128
                },
                RECT
            )
            .unwrap(),
            FULL
        );
        let crop = WindowRect {
            left: 1,
            top: 31,
            right: 99,
            bottom: 79,
        };
        assert_eq!(
            validate_crop(FULL, FULL, FULL, crop).unwrap(),
            CaptureSize {
                width: 98,
                height: 48
            }
        );
    }
    #[test]
    fn rejects_changed_content_and_small_texture() {
        assert!(validate_crop(
            FULL,
            CaptureSize {
                width: 99,
                height: 80
            },
            FULL,
            RECT
        )
        .is_err());
        assert!(validate_crop(
            FULL,
            FULL,
            CaptureSize {
                width: 99,
                height: 80
            },
            RECT
        )
        .is_err());
    }
    #[test]
    fn rejects_empty_negative_and_out_of_bounds_crops() {
        for crop in [
            WindowRect { left: -1, ..RECT },
            WindowRect { top: -1, ..RECT },
            WindowRect { right: 0, ..RECT },
            WindowRect { bottom: 0, ..RECT },
            WindowRect { right: 101, ..RECT },
            WindowRect { bottom: 81, ..RECT },
            WindowRect {
                left: i32::MIN,
                right: i32::MAX,
                ..RECT
            },
        ] {
            assert!(validate_crop(FULL, FULL, FULL, crop).is_err(), "{crop:?}");
        }
    }
    #[test]
    fn rejects_nonpositive_native_sizes() {
        for (width, height) in [(0, 1), (1, 0), (-1, 1), (1, -1)] {
            assert!(size(SizeInt32 {
                Width: width,
                Height: height
            })
            .is_err());
        }
    }
    #[test]
    fn decodes_channels_and_skips_row_padding() {
        let image = decode_rows(
            &[1, 2, 3, 4, 99, 99, 99, 99, 5, 6, 7, 8],
            CaptureSize {
                width: 1,
                height: 2,
            },
            8,
        )
        .unwrap();
        assert_eq!(image.to_rgba8().into_raw(), [3, 2, 1, 4, 7, 6, 5, 8]);
    }
    #[test]
    fn rejects_short_rows_buffers_and_overflow() {
        let size = CaptureSize {
            width: 2,
            height: 2,
        };
        assert!(row_layout(size, 7).is_err());
        assert!(row_layout(size, usize::MAX).is_err());
        assert!(decode_rows(&[0; 15], size, 8).is_err());
    }
    #[test]
    fn frame_wait_times_out_and_reports_disconnect() {
        let (sender, receiver) = sync_channel::<Result<()>>(1);
        assert!(matches!(
            receive(&receiver, Duration::from_millis(5)),
            Err(Error::CaptureTimeout(_))
        ));
        drop(sender);
        assert!(matches!(
            receive(&receiver, Duration::from_secs(1)),
            Err(Error::FrameChannelClosed)
        ));
    }
    #[test]
    fn frame_wait_propagates_worker_errors() {
        let (sender, receiver) = sync_channel::<Result<()>>(1);
        sender.send(Err(Error::TargetClosed)).unwrap();
        assert!(matches!(
            receive(&receiver, Duration::from_secs(1)),
            Err(Error::TargetClosed)
        ));
    }
}
