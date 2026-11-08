use std::{ffi::c_void, io::Cursor, ptr, ptr::NonNull};

use dispatch2::run_on_main;
use image::{ImageFormat, ImageReader, RgbaImage};
use objc2::{AnyThread, rc::Retained};
use objc2_app_kit::{NSApplication, NSImage, NSView};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo,
};
use objc2_foundation::NSSize;
use roboscope_ipc::display::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
use tracing::debug;
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

static APP_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../assets/mac/icon.png"
));

/// Performs one-time initialization (sets the app icon).
pub fn init_app() {
    let icon_image = ImageReader::with_format(Cursor::new(APP_ICON_PNG), ImageFormat::Png)
        .decode()
        .unwrap();

    set_app_icon(icon_image.into());
}

/// Frees the pixel buffer handed to `CGDataProviderCreateWithData` in [`set_app_icon`].
///
/// # Safety
///
/// `data` must be the pointer of a `Box<[u8]>` of length `size` that has been leaked with
/// [`Box::into_raw`], and must not be used again after this call.
unsafe extern "C-unwind" fn release_boxed_pixels(
    _info: *mut c_void,
    data: NonNull<c_void>,
    size: usize,
) {
    drop(unsafe {
        Box::from_raw(ptr::slice_from_raw_parts_mut(
            data.as_ptr().cast::<u8>(),
            size,
        ))
    });
}

/// Sets the app icon in the dock to the given image.
fn set_app_icon(icon: RgbaImage) {
    run_on_main(move |marker| {
        let (width, height) = (icon.width() as usize, icon.height() as usize);

        // AppKit re-renders the image periodically, so we need the image to outlive this function.
        let pixels: Box<[u8]> = icon.into_raw().into_boxed_slice();
        let len = pixels.len();
        let pixels = Box::into_raw(pixels).cast::<c_void>();

        let data_provider = unsafe {
            CGDataProvider::with_data(ptr::null_mut(), pixels, len, Some(release_boxed_pixels))
                .unwrap()
        };

        let image = unsafe {
            CGImage::new(
                width,
                height,
                8,
                8 * 4,
                4 * width,
                Some(&CGColorSpace::new_device_rgb().unwrap()),
                CGBitmapInfo(CGImageByteOrderInfo::Order32Big.0 | CGImageAlphaInfo::Last.0),
                Some(&data_provider),
                ptr::null(),
                true,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
            .unwrap()
        };

        let appkit_image = NSImage::initWithCGImage_size(
            NSImage::alloc(),
            &image,
            NSSize::new(width as f64, height as f64),
        );

        let app = NSApplication::sharedApplication(marker);

        unsafe {
            app.setApplicationIconImage(Some(&appkit_image));
        }
    });
}

/// Lock the window's aspect ratio to the size of the V5's display.
pub fn notify_aspect_ratio(window: &Window) {
    run_on_main(move |_| {
        let handle = window.window_handle().unwrap().as_raw();
        let RawWindowHandle::AppKit(handle) = handle else {
            panic!("Expected AppKit window")
        };

        let view: Retained<NSView> =
            unsafe { Retained::retain(handle.ns_view.as_ptr().cast()).unwrap() };
        let ns_window = view.window().unwrap();

        debug!("Setting window content aspect ratio");
        ns_window.setContentAspectRatio(NSSize {
            width: DISPLAY_WIDTH as f64,
            height: DISPLAY_HEIGHT as f64,
        });
    });
}
