//! Brain Display

use core::ffi::{VaList, c_char};
use std::{
    borrow::Cow,
    ffi::{CStr, CString},
    io::Cursor,
    mem::MaybeUninit,
    ptr,
};
use tracing::trace;

pub use vex_sdk::v5_image;

use crate::{
    canvas::{CANVAS, Canvas, DEFAULT_BLACK, DEFAULT_WHITE, HEADER_HEIGHT, Point, Rect, WIDTH},
    config::DisplayTheme,
    display::{DISPLAY, SimDisplay},
};

/// Set the foreground color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayForegroundColor(col: u32) {
    CANVAS.lock().state.fg_color = col;
}

/// Set the background color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayBackgroundColor(col: u32) {
    CANVAS.lock().state.bg_color = col;
}

/// Fill the screen with the default background color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayErase() {
    let mut canvas = CANVAS.lock();
    let original_fg_color = canvas.state.fg_color;

    canvas.state.fg_color = if canvas.theme() == DisplayTheme::Dark {
        DEFAULT_BLACK
    } else {
        DEFAULT_WHITE
    };
    canvas.fill_rect(Rect::FULL_CLIP);

    canvas.state.fg_color = original_fg_color;
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayScroll(nStartLine: i32, nLines: i32) {
    super::sdk_unimplemented!("vexDisplayScroll");
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayScrollRect(x1: i32, y1: i32, x2: i32, y2: i32, nLines: i32) {
    super::sdk_unimplemented!("vexDisplayScrollRect");
}

/// Copies pixels from the given buffer `pSrc` to the canvas.
///
/// When reading pixels, `pSrc` will be interpreted as containing `0RGB`-formatted pixel data, and
/// `srcStride` will be used to calculate the distance between rows in the source image (which can
/// be used to copy a cropped portion of a larger image onto the canvas).
///
/// # Safety
///
/// `pSrc` must be valid for reads from the following offsets: `0..=((y2-y1) * stride + (x2-x1))`.
///
/// `srcStride` must be nonzero, unless `y2` equals `y1`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayCopyRect(
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    pSrc: *mut u32,
    srcStride: i32,
) {
    unsafe {
        CANVAS.lock().copy_rect(
            Rect::from_sdk(x1, y1, x2, y2),
            pSrc,
            srcStride.max(0) as usize,
        );
    }
}

/// Write a pixel using the foreground color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayPixelSet(x: u32, y: u32) {
    let mut canvas = CANVAS.lock();
    canvas.set_pixel(Point {
        x: x as i32,
        y: y as i32 + HEADER_HEIGHT,
    });
}

/// Write a pixel using the background color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayPixelClear(x: u32, y: u32) {
    let mut canvas = CANVAS.lock();
    canvas.state.swap_colors();
    canvas.set_pixel(Point {
        x: x as i32,
        y: y as i32 + HEADER_HEIGHT,
    });
    canvas.state.swap_colors();
}

/// Draw a 1px-wide line using the foreground color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayLineDraw(x1: i32, y1: i32, x2: i32, y2: i32) {
    let mut canvas = CANVAS.lock();
    canvas.draw_line(
        Point {
            x: x1,
            y: y1 + HEADER_HEIGHT,
        },
        Point {
            x: x2,
            y: y2 + HEADER_HEIGHT,
        },
    );
}

/// Draw a 1px-wide line using the background color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayLineClear(x1: i32, y1: i32, x2: i32, y2: i32) {
    let mut canvas = CANVAS.lock();
    canvas.state.swap_colors();
    canvas.draw_line(
        Point {
            x: x1,
            y: y1 + HEADER_HEIGHT,
        },
        Point {
            x: x2,
            y: y2 + HEADER_HEIGHT,
        },
    );
    canvas.state.swap_colors();
}

/// Trace the 1px-wide outline of the given rectangle using the foreground color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayRectDraw(x1: i32, y1: i32, x2: i32, y2: i32) {
    let mut canvas = CANVAS.lock();
    canvas.draw_rect(Rect::from_sdk(x1, y1, x2, y2));
}

/// Fill the given rectangle of pixels with the background color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayRectClear(x1: i32, y1: i32, x2: i32, y2: i32) {
    let mut canvas = CANVAS.lock();
    canvas.state.swap_colors();
    canvas.fill_rect(Rect::from_sdk(x1, y1, x2, y2));
    canvas.state.swap_colors();
}

/// Fill the given rectangle of pixels with the foreground color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayRectFill(x1: i32, y1: i32, x2: i32, y2: i32) {
    let mut canvas = CANVAS.lock();
    canvas.fill_rect(Rect::from_sdk(x1, y1, x2, y2));
}

/// Trace the 1px-wide outline of the given circle using the foreground color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayCircleDraw(xc: i32, yc: i32, radius: i32) {
    let mut canvas = CANVAS.lock();

    let point = Point {
        x: xc,
        y: yc + HEADER_HEIGHT,
    };
    canvas.draw_circle(point, radius.max(0) as u32);
}

/// Fill the given circle of pixels with the background color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayCircleClear(xc: i32, yc: i32, radius: i32) {
    let mut canvas = CANVAS.lock();
    canvas.state.swap_colors();

    let point = Point {
        x: xc,
        y: yc + HEADER_HEIGHT,
    };
    canvas.fill_circle(point, radius.max(0) as u32);

    canvas.state.swap_colors();
}

/// Fill the given circle of pixels with the foreground color.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayCircleFill(xc: i32, yc: i32, radius: i32) {
    let mut canvas = CANVAS.lock();

    let point = Point {
        x: xc,
        y: yc + HEADER_HEIGHT,
    };
    canvas.fill_circle(point, radius.max(0) as u32);
}

/// Set the text scaling factor to the given fraction `n / d`.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayTextSize(n: u32, d: u32) {
    CANVAS.lock().state.font_scale = (n, d);
}

/// Set the current font by name.
///
/// The following font name aliases are supported by both the simulator and VEXos:
///
/// - `monospace`: "Noto Mono" at 49pt (default)
/// - `proportional`: "Noto Sans" at 54pt
///
/// The simulator supports these fonts names which don't work on VEXos:
///
/// - `NotoMono_49pt`: "Noto Mono" at 49pt, aka `monospace` (default)
/// - `NotoSans_54pt`: "Noto Sans" at 54pt, aka `proportional`
/// - `NotoMono_39pt`: "Noto Mono" at 39pt
/// - `Monospace_18pt`: "Monospace" at 18pt (this font is used by some VEXos system screens)
///
/// # Safety
///
/// The given font name must be a C string that's valid for reads.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayFontNamedSet(pFontName: *const c_char) {
    let c_str = unsafe { CStr::from_ptr(pFontName) };
    let str = c_str.to_str().unwrap_or_default();
    CANVAS.lock().state.set_named_font(str);
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayForegroundColorGet() -> u32 {
    CANVAS.lock().state.fg_color
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayBackgroundColorGet() -> u32 {
    CANVAS.lock().state.bg_color
}

/// Calculates the width in pixels of the given string using the current font and font scale.
///
/// # Safety
///
/// `pString` must be a C string that is valid for reads.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayStringWidthGet(pString: *const c_char) -> i32 {
    let c_str = unsafe { CStr::from_ptr(pString) };
    let str = c_str.to_string_lossy();
    CANVAS.lock().measure_string_width(&str)
}

/// Calculates the height in pixels of the given string using the current font and font scale.
///
/// # Safety
///
/// `pString` must be a C string that is valid for reads.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayStringHeightGet(pString: *const c_char) -> i32 {
    let c_str = unsafe { CStr::from_ptr(pString) };
    let str = c_str.to_string_lossy();
    CANVAS.lock().measure_string_height(&str)
}

/// This function saves the given value and returns it from [`vexDisplayPenSizeGet`].
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayPenSizeSet(width: u32) {
    CANVAS.lock().state.pen_size = width;
}

/// Returns the last value passed to [`vexDisplayPenSizeSet`].
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayPenSizeGet() -> u32 {
    CANVAS.lock().state.pen_size
}

/// Sets the rectangle inside which changes to the canvas are allowed.
///
/// The default clip region is the entire canvas. Note that unless "fullscreen" mode is enabled in
/// the simulator config, changes made underneath the header at the top of the screen will
/// not be visible.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayClipRegionSet(x1: i32, y1: i32, x2: i32, y2: i32) {
    let region = Rect::from_sdk(x1, y1, x2, y2);
    CANVAS.lock().state.set_clip_region(region);
}

/// Renders the contents of the user canvas on the simulated display.
///
/// Callers should note that rendering happens automatically on each frame (i.e. at 60 FPS) until
/// the first time this function is called, at which point changes to the canvas only appear on the
/// screen after subsequent calls to this function. Automatic rendering can be re-enabled by calling
/// [`vexDisplayDoubleBufferDisable`].
///
/// If `bVsyncWait` is `true`, the function will sleep until at least one frame showing the rendered
/// contents of the canvas has been committed.
///
/// The `bRunScheduler` parameter is currently a no-op.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayRender(bVsyncWait: bool, bRunScheduler: bool) {
    trace!("Dispatching render");

    let do_render = |display: &mut SimDisplay| {
        display.autorender = false;
        display.render_user_canvas(&CANVAS.lock());
        // We do not send an event to the renderer telling it to render because that could
        // potentially cause render speeds of more than 60fps which is not true to the V5 hardware.
    };

    if bVsyncWait {
        SimDisplay::run_synced(do_render);
    } else {
        do_render(&mut DISPLAY.lock());
    }
}

/// Re-enables automatic 60 FPS rendering after a call to [`vexDisplayRender`].
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayDoubleBufferDisable() {
    let mut display = DISPLAY.lock();
    display.autorender = true;
}

/// Unimplemented.
#[unsafe(no_mangle)]
pub extern "system" fn vexDisplayClipRegionSetWithIndex(
    index: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
) {
    super::sdk_unimplemented!("vexDisplayClipRegionSetWithIndex");
    unimplemented!("VEXos task api")
}

/// Unimplemented.
///
/// # Safety
///
/// N/A
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexImageBmpRead(
    ibuf: *const u8,
    oBuf: *mut v5_image,
    maxw: u32,
    maxh: u32,
) -> u32 {
    super::sdk_unimplemented!("vexImageBmpRead");
    // Unimplemented to avoid bringing in a BMP decoder that will probably not be used.
    // PNG is implemented because we use PNGs anyways.
    0
}

/// Decodes the given PNG data into an image that may be used with [`vexDisplayCopyRect`].
///
/// The function returns 1 if the operation succeeded or 0 if it failed.
///
/// If the operation succeeded, the decoded PNG data will be written to the beginning of
/// `(*oBuf).data`. `(*oBuf).width` and `(*oBuf).height` will also be updated to the width and
/// height of the pixel data written to `(*oBuf).data`.
///
/// # Safety
///
/// - `ibuf` must point to a buffer of at least length `ibuflen` that is valid for reads or it must
///   be null.
/// - `oBuf` must point to an initialized `v5_image` struct that is valid for writes or it must be
///   null.
/// - If `oBuf` is not null, `(*oBuf).data` must either point to a image buffer that is valid for
///   writes and at least `maxw * maxh * 4` bytes long or be null.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexImagePngRead(
    ibuf: *const u8,
    oBuf: *mut v5_image,
    maxw: u32,
    maxh: u32,
    ibuflen: u32,
) -> u32 {
    if ibuf.is_null() || oBuf.is_null() {
        return 0;
    }

    if (unsafe { (*oBuf).data }).is_null() {
        return 0;
    }

    let ibuf = unsafe { std::slice::from_raw_parts(ibuf, ibuflen as usize) };
    let decoder = png::Decoder::new(Cursor::new(ibuf));

    let mut reader = decoder.read_info().unwrap();
    let Some(size) = reader.output_buffer_size() else {
        return 0;
    };

    let output_buffer_bytes = (maxw * maxh * 4) as usize;
    if size > output_buffer_bytes {
        return 0;
    }

    let output_buffer =
        unsafe { std::slice::from_raw_parts_mut((*oBuf).data.cast::<u8>(), output_buffer_bytes) };
    let Ok(info) = reader.next_frame(output_buffer) else {
        return 0;
    };

    if let Ok(height) = u16::try_from(info.height)
        && let Ok(width) = u16::try_from(info.width)
    {
        unsafe {
            (*oBuf).width = width;
            (*oBuf).height = height;
        }
    } else {
        return 0;
    }

    1
}

unsafe fn format_string<'a>(
    buffer: &'a mut MaybeUninit<[u8; 256]>,
    format: *const c_char,
    args: VaList<'_>,
) -> Cow<'a, str> {
    let buffer = unsafe {
        super::vsnprintf(buffer.as_mut_ptr().cast(), 256, format, args);
        buffer.assume_init_mut()
    };

    let c_str = CStr::from_bytes_until_nul(buffer).unwrap();
    c_str.to_string_lossy()
}

fn line_to_y(line: i32) -> i32 {
    line * 20 + 2
}

/// Variant of [`vexDisplayPrintf`] which accepts a [`VaList`].
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayVPrintf(
    xpos: i32,
    ypos: i32,
    bOpaque: i32,
    format: *const c_char,
    args: VaList<'_>,
) {
    let mut buffer = MaybeUninit::uninit();
    let string = unsafe { format_string(&mut buffer, format, args) };

    let mut canvas = CANVAS.lock();
    canvas.draw_string(
        Point {
            x: xpos,
            y: ypos + HEADER_HEIGHT,
        },
        string.as_ref(),
        bOpaque != 0,
    );
}

/// Variant of [`vexDisplayString`] which accepts a [`VaList`].
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayVString(
    nLineNumber: i32,
    format: *const c_char,
    args: VaList<'_>,
) {
    unsafe {
        vexDisplayVStringAt(0, line_to_y(nLineNumber), format, args);
    }
}

/// Variant of [`vexDisplayStringAt`] which accepts a [`VaList`].
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayVStringAt(
    xpos: i32,
    ypos: i32,
    format: *const c_char,
    args: VaList<'_>,
) {
    let mut buffer = MaybeUninit::uninit();
    let string = unsafe { format_string(&mut buffer, format, args) };

    let mut canvas = CANVAS.lock();
    canvas.state.set_named_font("NotoMono_49pt");
    canvas.state.font_scale = (1, 3);

    canvas.draw_string(
        Point {
            x: xpos,
            y: ypos + HEADER_HEIGHT,
        },
        string.as_ref(),
        true,
    );
}

/// Variant of [`vexDisplayVBigString`] which accepts a [`VaList`].
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayVBigString(
    nLineNumber: i32,
    format: *const c_char,
    args: VaList<'_>,
) {
    unsafe {
        vexDisplayVBigStringAt(0, line_to_y(nLineNumber), format, args);
    }
}

/// Variant of [`vexDisplayBigStringAt`] which accepts a [`VaList`].
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayVBigStringAt(
    xpos: i32,
    ypos: i32,
    format: *const c_char,
    args: VaList<'_>,
) {
    let mut buffer = MaybeUninit::uninit();
    let string = unsafe { format_string(&mut buffer, format, args) };

    let mut canvas = CANVAS.lock();
    canvas.state.set_named_font("NotoMono_49pt");
    canvas.state.font_scale = (2, 3);

    canvas.draw_string(
        Point {
            x: xpos,
            y: ypos + HEADER_HEIGHT,
        },
        string.as_ref(),
        true,
    );
}

/// Variant of [`vexDisplaySmallStringAt`] which accepts a [`VaList`].
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayVSmallStringAt(
    xpos: i32,
    ypos: i32,
    format: *const c_char,
    args: VaList<'_>,
) {
    let mut buffer = MaybeUninit::uninit();
    let string = unsafe { format_string(&mut buffer, format, args) };

    let mut canvas = CANVAS.lock();
    canvas.state.set_named_font("NotoMono_39pt");
    canvas.state.font_scale = (1, 3);

    canvas.draw_string(
        Point {
            x: xpos,
            y: ypos + HEADER_HEIGHT,
        },
        string.as_ref(),
        true,
    );
}

/// Variant of [`vexDisplayCenteredString`] which accepts a [`VaList`].
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayVCenteredString(
    nLineNumber: i32,
    format: *const c_char,
    args: VaList<'_>,
) {
    let mut buffer = MaybeUninit::uninit();
    let string = unsafe { format_string(&mut buffer, format, args) };

    let mut canvas = CANVAS.lock();
    canvas.state.set_named_font("NotoMono_49pt");
    canvas.state.font_scale = (1, 3);

    let str_width = canvas.measure_string_width(&string);

    canvas.draw_string(
        Point {
            x: (WIDTH as i32 - str_width) / 2,
            y: line_to_y(nLineNumber) + HEADER_HEIGHT,
        },
        string.as_ref(),
        true,
    );
}

/// Variant of [`vexDisplayBigCenteredString`] which accepts a [`VaList`].
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDisplayVBigCenteredString(
    nLineNumber: i32,
    format: *const c_char,
    args: VaList<'_>,
) {
    let mut buffer = MaybeUninit::uninit();
    let string = unsafe { format_string(&mut buffer, format, args) };

    let mut canvas = CANVAS.lock();
    canvas.state.set_named_font("NotoMono_49pt");
    canvas.state.font_scale = (2, 3);

    let str_width = canvas.measure_string_width(&string);

    canvas.draw_string(
        Point {
            x: (WIDTH as i32 - str_width) / 2,
            y: line_to_y(nLineNumber) + HEADER_HEIGHT,
        },
        string.as_ref(),
        true,
    );
}

/// Performs printf-style formatting and writes the result to the canvas at the given coordinates.
///
/// The coordinates passed to this function specify the upper-left corner of the drawn text. If
/// `bOpaque` is specified, a rectangle is filled behind the text using the current background
/// color.
///
/// The size and font of the text can be controlled with [`vexDisplayFontNamedSet`] and
/// [`vexDisplayTextSize`].
///
/// # Limitations
///
/// No more than 256 bytes of data can be written to the screen at once (any more will be
/// truncated). Only ASCII-formatted string data with characters between codepoints 32 and 126 is
/// supported. Unsupported characters will be replaced with a placeholder character.
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexDisplayPrintf(
    xpos: i32,
    ypos: i32,
    bOpaque: i32,
    format: *const c_char,
    args: ...
) {
    unsafe { vexDisplayVPrintf(xpos, ypos, bOpaque, format, args) }
}

/// Performs printf-style formatting and writes the result to the canvas on the given line.
///
/// The text is drawn with the "normal" font size. See [`vexDisplayPrintf`] for more information.
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexDisplayString(nLineNumber: i32, format: *const c_char, args: ...) {
    unsafe { vexDisplayVString(nLineNumber, format, args) }
}

/// Performs printf-style formatting and writes the result to the canvas at the given coordinates.
///
/// The text is drawn with the "normal" font size. See [`vexDisplayPrintf`] for more information.
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexDisplayStringAt(
    xpos: i32,
    ypos: i32,
    format: *const c_char,
    args: ...
) {
    unsafe { vexDisplayVStringAt(xpos, ypos, format, args) }
}

/// Performs printf-style formatting and writes the result to the canvas on the given line.
///
/// The text is drawn with the "big" font size. See [`vexDisplayPrintf`] for more information.
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexDisplayBigString(nLineNumber: i32, format: *const c_char, args: ...) {
    unsafe { vexDisplayVBigString(nLineNumber, format, args) }
}

/// Performs printf-style formatting and writes the result to the canvas at the given coordinates.
///
/// The text is drawn with the "big" font size. See [`vexDisplayPrintf`] for more information.
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexDisplayBigStringAt(
    xpos: i32,
    ypos: i32,
    format: *const c_char,
    args: ...
) {
    unsafe { vexDisplayVBigStringAt(xpos, ypos, format, args) }
}

/// Performs printf-style formatting and writes the result to the canvas at the given coordinates.
///
/// The text is drawn with the "small" font size. See [`vexDisplayPrintf`] for more information.
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexDisplaySmallStringAt(
    xpos: i32,
    ypos: i32,
    format: *const c_char,
    args: ...
) {
    unsafe { vexDisplayVSmallStringAt(xpos, ypos, format, args) }
}

/// Performs printf-style formatting and writes the result to the canvas on the given line in the
/// horizontal center of the screen.
///
/// The text is drawn with the "normal" font size. See [`vexDisplayPrintf`] for more information.
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexDisplayCenteredString(
    nLineNumber: i32,
    format: *const c_char,
    args: ...
) {
    unsafe { vexDisplayVCenteredString(nLineNumber, format, args) }
}

/// Performs printf-style formatting and writes the result to the canvas on the given line in the
/// horizontal center of the screen.
///
/// The text is drawn with the "big" font size. See [`vexDisplayPrintf`] for more information.
///
/// # Safety
///
/// See [`vexDisplayPrintf`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexDisplayBigCenteredString(
    nLineNumber: i32,
    format: *const c_char,
    args: ...
) {
    unsafe { vexDisplayVBigCenteredString(nLineNumber, format, args) }
}
