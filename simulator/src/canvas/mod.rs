use std::{
    fmt::{self, Formatter},
    io::Cursor,
    mem,
    ops::RangeInclusive,
    path::Path,
    sync::{Arc, LazyLock},
};

use fast_image_resize::{
    images::{TypedCroppedImageMut, TypedImage},
    pixels::U8,
};
use image::{GrayImage, ImageFormat, ImageReader};
use line_drawing::{Bresenham, BresenhamCircle};
use parking_lot::Mutex;
use tracing::trace;

use crate::{
    canvas::font::{FONTS, PreRenderedFont},
    config::{DebugFlag, DisplayTheme, config},
};

mod font;

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 272;
pub const HEADER_HEIGHT: i32 = 32;
pub const BUFSZ: usize = WIDTH as usize * HEIGHT as usize;

const TEXT_WIDTH: u32 = 512;
const TEXT_BUFSZ: usize = TEXT_WIDTH as usize * TEXT_WIDTH as usize;

pub const DEFAULT_BLACK: u32 = 0x00_00_00;
pub const DEFAULT_WHITE: u32 = 0xFF_FF_FF;

/// The canvas instance used by user code.
pub static CANVAS: LazyLock<Mutex<Canvas>> = LazyLock::new(Mutex::default);

#[derive(Debug, Clone)]
pub struct CanvasState {
    pub fg_color: u32,
    pub bg_color: u32,
    // This doesn't seem to affect any operations, but we store it so we can return it from
    // `vexDisplayPenSizeGet`.
    pub pen_size: u32,

    /// The region where changes to the canvas are allowed.
    clip_region: Rect,

    /// A set of glyphs rendered at a certain point size which can be scaled up or down to draw
    /// text.
    pub font: Arc<PreRenderedFont>,

    /// The fractional scale factor of drawn fonts, stored as a numerator and denominator.
    ///
    /// All font glyphs are pre-rendered at full scale (i.e. 1/1 scale) and can be scaled to a
    /// desired size such as half (1/2) scale or one-third (1/3) scale. The default scale factor is
    /// one-third (1/3) of the full size.
    pub font_scale: (u32, u32),
}

impl CanvasState {
    pub fn swap_colors(&mut self) {
        mem::swap(&mut self.fg_color, &mut self.bg_color);
    }

    pub fn set_clip_region(&mut self, mut region: Rect) {
        region.clip_to(Rect::FULL_CLIP);
        self.clip_region = region;
    }

    /// Set the current font by name.
    ///
    /// If the font doesn't exist, this function is a no-op.
    pub fn set_named_font(&mut self, name: &str) {
        if let Some(font) = FONTS.get(name) {
            self.font = font;
        }
    }
}

pub struct Canvas {
    /// Primary image buffer.
    buffer: Box<[u32; BUFSZ]>,
    /// Scratch buffer for planning text writes before compositing them onto the main buffer. Holds
    /// opacity values for the text.
    text_buffer: Box<[u8; TEXT_BUFSZ]>,
    theme: DisplayTheme,
    pub state: CanvasState,
}

impl Canvas {
    pub fn new(theme: DisplayTheme) -> Self {
        let mut state = CanvasState {
            fg_color: DEFAULT_WHITE,
            bg_color: DEFAULT_BLACK,
            clip_region: Rect::FULL_CLIP,
            pen_size: 1,
            font: FONTS.get("monospace").unwrap(),
            font_scale: (1, 3),
        };

        if theme == DisplayTheme::Light {
            mem::swap(&mut state.fg_color, &mut state.bg_color);
        }

        Self {
            // Allocate directly on the heap to prevent a stack overflow.
            buffer: vec![0u32; BUFSZ].into_boxed_slice().try_into().unwrap(),
            text_buffer: vec![0u8; TEXT_BUFSZ].into_boxed_slice().try_into().unwrap(),
            state,
            theme,
        }
    }

    pub const fn theme(&self) -> DisplayTheme {
        self.theme
    }

    /// Set the given pixel to the foreground color.
    pub fn set_pixel(&mut self, point: Point) {
        if !point.is_inside(self.state.clip_region) {
            return;
        }

        trace!(color = %Hex(self.state.fg_color), ?point, "update pixel");
        self.write_pixel(point, self.state.fg_color);
    }

    /// Set the given pixel to the given color, without performing bounds/clip region checks.
    fn write_pixel(&mut self, point: Point, color: u32) {
        let idx = point.y * WIDTH as i32 + point.x;
        self.buffer[idx as usize] = color;
    }

    /// Draw a horizontal line using the foreground color.
    pub fn draw_horizontal_line(&mut self, x_range: RangeInclusive<i32>, y: i32) {
        trace!(?x_range, y, "horizontal line");

        let clip = self.state.clip_region;

        // Is the line above or below the clip region?
        if !(clip.0.y..clip.1.y).contains(&y) {
            return;
        }

        // Try to clamp to the horizontal clip region or bail out if it's totally out of range.
        let allowed_x_range = clip.0.x..=(clip.1.x - 1);
        let Some(x_range) = clamp_range(x_range, allowed_x_range) else {
            return;
        };

        for x in x_range {
            self.write_pixel(Point { x, y }, self.state.fg_color);
        }
    }

    /// Draw a vertical line using the foreground color.
    pub fn draw_vertical_line(&mut self, x: i32, y_range: RangeInclusive<i32>) {
        trace!(x, ?y_range, "vertical line");

        let clip = self.state.clip_region;

        // Is the line left or right of the clip region?
        if !(clip.0.x..clip.1.x).contains(&x) {
            return;
        }

        // Try to clamp to the vertical clip region or bail out if it's totally out of range.
        let allowed_y_range = clip.0.y..=(clip.1.y - 1);
        let Some(y_range) = clamp_range(y_range, allowed_y_range) else {
            return;
        };

        for y in y_range {
            self.write_pixel(Point { x, y }, self.state.fg_color);
        }
    }

    /// Draw a line using the foreground color with Bresenham's line algorithm.
    pub fn draw_line(&mut self, start: Point, end: Point) {
        trace!(?start, ?end, "line");

        for (x, y) in Bresenham::new((start.x, start.y), (end.x, end.y)) {
            self.set_pixel(Point { x, y });
        }
    }

    /// Fill in the given rectangle with the foreground color.
    pub fn fill_rect(&mut self, mut bounds: Rect) {
        trace!(color = %Hex(self.state.fg_color), ?bounds, "fill rect");

        bounds.clip_to(self.state.clip_region);

        for pixel in bounds.pixels() {
            self.write_pixel(pixel, self.state.fg_color);
        }
    }

    /// Draw the borders of the given rectangle with the foreground color.
    pub fn draw_rect(&mut self, bounds: Rect) {
        trace!(color = %Hex(self.state.fg_color), ?bounds, "trace rect");

        let horizontal_lines = [bounds.0.y, bounds.1.y - 1];
        let vertical_lines = [bounds.0.x, bounds.1.x - 1];

        for y in horizontal_lines {
            self.draw_horizontal_line(bounds.0.x..=(bounds.1.x - 1), y);
        }

        for x in vertical_lines {
            self.draw_vertical_line(x, bounds.0.y..=(bounds.1.y - 1));
        }
    }

    /// Fill in the given circle with the foreground color using Bresenham's Circle algorithm.
    pub fn fill_circle(&mut self, center: Point, radius: u32) {
        trace!(color = %Hex(self.state.fg_color), ?center, radius, "fill circle");

        // Special case to treat radius zero as a set_pixel call since using Bresenham would just
        // give us an empty iterator.
        if radius == 0 {
            self.set_pixel(center);
        }

        // Turn the circle into a bunch of horizontal lines by using Bresenham's circle
        // algorithm to find the left and right extents of each line.

        // The center point isn't included in the radius, so it gets its own extra line.
        let num_lines = 1 + radius * 2;
        let mut lines = vec![(center.x, center.x); num_lines as usize];

        for (dx, i) in BresenhamCircle::new(0, radius as i32, radius as i32) {
            let x = center.x + dx;

            // The tops and bottoms of circles have several points on the same line, so only record
            // the leftmost or rightmost point for our horizontal line.
            if dx < 0 {
                if x < lines[i as usize].0 {
                    lines[i as usize].0 = x;
                }
            } else if x > lines[i as usize].1 {
                lines[i as usize].1 = x;
            }
        }

        // Iterate through each line and draw it.
        for (line, (left, right)) in lines.into_iter().enumerate() {
            let y = center.y - radius as i32 + line as i32;
            self.draw_horizontal_line(left..=right, y);
        }
    }

    /// Draw the borders of the given circle with the foreground color using Bresenham's Circle
    /// algorithm.
    pub fn draw_circle(&mut self, center: Point, radius: u32) {
        trace!(color = %Hex(self.state.fg_color), ?center, radius, "trace circle");

        // Special case to treat radius zero as a set_pixel call since using Bresenham would just
        // give us an empty iterator.
        if radius == 0 {
            self.set_pixel(center);
        }

        let clip = self.state.clip_region;

        for (x, y) in BresenhamCircle::new(center.x, center.y, radius as i32) {
            if (Point { x, y }).is_inside(clip) {
                self.write_pixel(Point { x, y }, self.state.fg_color);
            }
        }
    }

    /// Copy the source buffer onto the display buffer.
    pub unsafe fn copy_rect(&mut self, mut bounds: Rect, source: *const u32, stride: usize) {
        trace!(?bounds, ?source, ?stride, "copy rect");
        let origin = bounds.0;
        bounds.clip_to(self.state.clip_region);

        // When the top/left of the bounds is clipped off, begin part-way through the
        // source image rather than showing the beginning of it lower.
        let col_offset = (bounds.0.x - origin.x) as usize;
        let row_offset = (bounds.0.y - origin.y) as usize;

        for (row_idx, row) in (bounds.0.y..bounds.1.y).enumerate() {
            for (col_idx, col) in (bounds.0.x..bounds.1.x).enumerate() {
                let dest_idx = row * WIDTH as i32 + col;
                let source_idx = (row_offset + row_idx) * stride + (col_offset + col_idx);
                let pixel = unsafe { source.add(source_idx).read() };
                self.buffer[dest_idx as usize] = pixel;
            }
        }
    }

    /// Draw the given string on the display in the foreground color.
    ///
    /// If `opaque` is specified, the text will be highlighted with the background color.
    pub fn draw_string(&mut self, origin: Point, string: &str, opaque: bool) {
        let font = self.state.font.clone();

        let (numerator, denominator) = self.state.font_scale;
        let ascent = font.ascent(numerator, denominator);
        let height = font.height(numerator, denominator);

        trace!(
            ?string,
            ?origin,
            color = %Hex(self.state.fg_color),
            font_name = ?font.name(),
            ?ascent,
            ?height,
            "Rendering string"
        );

        let mut x_cursor = 0;

        self.text_buffer.fill(0);
        let mut text_destination: TypedImage<U8> =
            TypedImage::from_buffer(TEXT_WIDTH, TEXT_WIDTH, &mut *self.text_buffer).unwrap();

        // Create an opacity mask by rendering each character at the desired size in the text
        // scratch buffer.

        for character in string.chars() {
            let glyph = font.glyph_for_char(character);

            // Get the size and visual offset of this character, scaled as needed.
            let bounds = glyph.scaled_raster_bounds(numerator, denominator);

            trace!(?character, ?x_cursor, ?bounds, "rendering character");

            let mut dest_bounds = Rect::sized(
                x_cursor + bounds.origin_x(),
                bounds.origin_y() + ascent as i32,
                bounds.width(),
                bounds.height(),
            );

            // Make sure we're not writing out of the bounds of text scratch buffer.
            // If this clip takes effect, the resize will probably make the glyph look stretched,
            // but the scratch buffer is big enough that it shouldn't be an issue.
            dest_bounds.clip_to(Rect::new(0, 0, 512, 512));

            // The offset of `bounds` here offsets the character so it appears to sit on the
            // baseline, and adding `ascent` makes it so the top-left of the character is the
            // origin rather than the bottom right.
            let mut glyph_destination = TypedCroppedImageMut::from_ref(
                &mut text_destination,
                dest_bounds.left() as u32,
                dest_bounds.top() as u32,
                dest_bounds.width() as u32,
                dest_bounds.height() as u32,
            )
            .unwrap();

            // The glyph will be scaled up or down to fill `glyph_destination`.
            glyph.render(&mut glyph_destination);

            x_cursor += glyph.advance(numerator, denominator);
        }

        if config().debug.contains(&DebugFlag::TextBuffer) {
            self.save_text_buffer_png("text_mask.png").unwrap();
        }

        // Now that we've set up our mask, we need to apply it on the real canvas.

        let mut dest_bounds = Rect::sized(origin.x, origin.y, x_cursor, height as i32);
        dest_bounds.clip_to(self.state.clip_region);

        if opaque {
            self.state.swap_colors();
            self.fill_rect(dest_bounds);
            self.state.swap_colors();
        }

        let [_, cr, cg, cb] = self.state.fg_color.to_be_bytes();

        for Point { x, y } in dest_bounds.pixels() {
            let dest_idx = y * WIDTH as i32 + x;
            let src_idx = (y - origin.y) * 512 + (x - origin.x);

            let opacity = self.text_buffer[src_idx as usize];
            let color = &mut self.buffer[dest_idx as usize];

            let [_, r, g, b] = color.to_be_bytes();
            let transparency = (255 - opacity) as u32;

            // Alpha is 0..=255 instead of 0..=1 so we need to divide by 255 to keep the same scale.
            // This is done at the end to make the integer multiplication more accurate.
            let r = ((r as u32 * transparency) + (cr as u32 * opacity as u32)) / 255;
            let g = ((g as u32 * transparency) + (cg as u32 * opacity as u32)) / 255;
            let b = ((b as u32 * transparency) + (cb as u32 * opacity as u32)) / 255;

            *color = u32::from_be_bytes([0, r as u8, g as u8, b as u8]);
        }
    }

    /// Get the height of the given string in pixels.
    pub fn measure_string_height(&mut self, _string: &str) -> i32 {
        let (numerator, denominator) = self.state.font_scale;
        self.state.font.height(numerator, denominator) as i32
    }

    /// Get the width of the given string in pixels.
    pub fn measure_string_width(&mut self, string: &str) -> i32 {
        let (numerator, denominator) = self.state.font_scale;
        let font = self.state.font.clone();

        let mut width = 0;
        for character in string.chars() {
            let glyph = font.glyph_for_char(character);
            width += glyph.advance(numerator, denominator);
        }

        width
    }

    /// Get a reference to the raw pixel data of the buffer.
    pub fn buffer(&self) -> &[u32; BUFSZ] {
        &self.buffer
    }

    /// Save the contents of the text buffer as a grayscale PNG for debugging.
    pub fn save_text_buffer_png(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        GrayImage::from_raw(TEXT_WIDTH, TEXT_WIDTH, self.text_buffer.to_vec())
            .expect("text_buffer dimensions are always valid")
            .save(path.as_ref())?;
        Ok(())
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new(config().theme)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    // These are signed so you can do things like drawing circles and lines with parts off the
    // left side of the screen (obviously they will be clipped, but the part that's on the screen
    // should work properly).
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    fn clamp_to(&mut self, region: Rect) {
        self.x = self.x.clamp(region.0.x, region.1.x - 1);
        self.y = self.y.clamp(region.0.y, region.1.y - 1);
    }

    fn is_inside(&self, region: Rect) -> bool {
        (region.0.x..region.1.x).contains(&self.x) && (region.0.y..region.1.y).contains(&self.y)
    }
}

/// A rectangle represented as an upper-left and a bottom-right point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect(pub Point, pub Point);

impl Rect {
    pub const FULL_CLIP: Self = Rect::new(0, 0, WIDTH as i32, HEIGHT as i32);
    pub const USER_CLIP: Self = Rect::new(0, HEADER_HEIGHT, WIDTH as i32, HEIGHT as i32);
    pub const HEADER_CLIP: Self = Rect::new(0, 0, WIDTH as i32, HEADER_HEIGHT);

    /// Create a new rectangle from two corner points diagonally opposite from each other.
    ///
    /// The order of the points will be normalized to (upper left, bottom right). The bottom right
    /// point is not included in the rectangle.
    pub const fn new(mut x0: i32, mut y0: i32, mut x1: i32, mut y1: i32) -> Self {
        if x0 > x1 {
            mem::swap(&mut x0, &mut x1);
        }
        if y0 > y1 {
            mem::swap(&mut y0, &mut y1);
        }

        Self(Point { x: x0, y: y0 }, Point { x: x1, y: y1 })
    }

    /// Create a new rectangle from two corner points which are both included in the rectangle.
    ///
    /// This constructor cannot be used to create a rectangle without any area.
    pub fn from_sdk(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let mut rect = Self::new(x0, y0 + HEADER_HEIGHT, x1, y1 + HEADER_HEIGHT);
        rect.1.x += 1;
        rect.1.y += 1;
        rect
    }

    /// Create a new rectangle using its upper-left point and size.
    pub const fn sized(x0: i32, y0: i32, width: i32, height: i32) -> Self {
        Self(
            Point { x: x0, y: y0 },
            Point {
                x: x0 + width,
                y: y0 + height,
            },
        )
    }

    /// Get the distance from the left side of the screen.
    pub fn left(&self) -> i32 {
        self.0.x
    }

    /// Get the distance from the top of the screen.
    pub fn top(&self) -> i32 {
        self.0.y
    }

    /// Get the width of the rectangle.
    pub fn width(&self) -> i32 {
        self.1.x - self.0.x
    }

    /// Get the height of the rectangle.
    pub fn height(&self) -> i32 {
        self.1.y - self.0.y
    }

    /// Shrink this rectangle to be completely enclosed by the given other rectangle.
    pub fn clip_to(&mut self, region: Rect) {
        self.0.clamp_to(region);
        self.1.clamp_to(region);
    }

    /// Iterate over the pixels in this rectangle, row-by-row.
    pub fn pixels(&self) -> impl Iterator<Item = Point> {
        (self.0.y..self.1.y).flat_map(|y| (self.0.x..self.1.x).map(move |x| Point { x, y }))
    }
}

struct Hex(u32);
impl std::fmt::Display for Hex {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "#{:x?}", self.0)
    }
}

/// Clamps `source` to the range `region`, or returns `None` if source is completely outside
/// `region`.
///
/// # Panics
///
/// Panics if `start` < `end` for either `source` or `region`.
fn clamp_range<T: PartialOrd + Copy>(
    source: RangeInclusive<T>,
    region: RangeInclusive<T>,
) -> Option<RangeInclusive<T>> {
    assert!(source.start() <= source.end());
    assert!(region.start() <= region.end());

    let mut begin = *source.start();
    let mut end = *source.end();

    let region_begin = *region.start();
    let region_end = *region.end();

    if begin > region_end || end < region_begin {
        return None;
    }

    if end > region_end {
        end = region_end;
    }
    if begin < region_begin {
        begin = region_begin;
    }

    Some(begin..=end)
}

pub struct SimImage {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
}

impl SimImage {
    pub fn from_png(png_bytes: &[u8]) -> Self {
        let img = ImageReader::with_format(Cursor::new(png_bytes), ImageFormat::Png)
            .decode()
            .unwrap()
            .into_rgb8();

        let width = img.width();
        let height = img.height();

        // We use u32 colors, not u8x4/u8x3, so we need to align the pixels to 4 bytes.
        let pixels: Vec<u32> = img
            .pixels()
            .map(|p| u32::from_be_bytes([0, p[0], p[1], p[2]]))
            .collect();

        Self {
            pixels,
            width,
            height,
        }
    }

    pub fn width(&self) -> i32 {
        self.width as i32
    }

    pub fn draw(&self, canvas: &mut Canvas, origin: Point) {
        let bounds = Rect::sized(origin.x, origin.y, self.width as i32, self.height as i32);
        unsafe {
            canvas.copy_rect(bounds, self.pixels.as_ptr(), self.width as usize);
        };
    }
}
