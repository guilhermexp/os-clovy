//! fal image editing through the shared queue transport in
//! [`crate::fal_image`].
//!
//! The source image travels inline as a data URI in `image_urls`, so Clovy
//! never uploads the user's picture to fal's public CDN. The edited image is
//! resolved exactly like a generated one: inline PNG or a capped,
//! credential-free `*.fal.media` download, sniffed format, and
//! `has_nsfw_concepts` treated as a rejection.
//!
//! `fal-ai/flux-2/edit` is the curated edit endpoint. `safe_mode` maps to its
//! boolean `enable_safety_checker`: `true` asks for the check, while `false`
//! takes effect only for fal accounts authorized to disable it (others are
//! always checked), so turning it off is a request, never a guaranteed bypass.
//!
//! Geometry. The source's header is read with the `image` crate (format,
//! size, EXIF orientation) without decoding its pixels, except that a JPEG
//! decoder reads the whole file before its header. The output is requested
//! explicitly as whole pixels: both sides within the schema's 512..=2048, at
//! most 1024x1024 pixels in total, the long side equal to the source's
//! long:short ratio (clamped to 4, the ratio of those limits) times the short
//! side, to the nearest pixel. The source sent stays within the same pixel
//! budget on every path.
//!
//! - Normal case (ratio at most 4, no orientation to apply), within the
//!   budget: the source goes exactly as received.
//! - Normal case over the budget: the whole source is scaled to exactly the
//!   requested size, its ratio kept to the nearest pixel, and sent as PNG;
//!   nothing is cut or padded.
//! - In both normal cases the result's header must report the requested
//!   size; any other size is a failed edit, never reinterpreted or cropped.
//! - Canvas case (ratio above 4, or an orientation to apply): the whole
//!   source, upright, is scaled without stretching into a centered rectangle
//!   of a neutral gray canvas of exactly the requested size. The result must
//!   come back at that size and is cropped to that rectangle. The crop assumes
//!   the model keeps the input's layout; fal does not document that, so it is
//!   a hypothesis for the external proof, and the size check proves geometry
//!   only, not content.
//!
//! Blocking work. Header reads, decoding, scaling, encoding and the result's
//! checks run on blocking threads that own the source text and the result
//! bytes (moved there, never copied whole). Decoding keeps the `image`
//! crate's default limits (an allocation over 512 MiB in 0.25 is
//! `image_edit_source_too_large`). Dropping the request, or its task, does
//! not stop a codec call already running: the thread finishes it, bounded by
//! the request body limit and those limits, and the result is discarded. No
//! format is read in constant memory: a JPEG header read holds the decoded
//! file once, and other formats' read patterns are not measured.
//!
//! Price: the model page bills $0.012 per megapixel of input and of output,
//! resizes the input to 1 MP, and prices a 1024x1024 output with a resized
//! input at $0.024 (<https://fal.ai/models/fal-ai/flux-2/edit/llms.txt>,
//! fetched 2026-10-08): an official example, not an observed charge. Clovy
//! sends and requests at most 1024x1024 pixels; since the megapixel unit and
//! rounding are not documented, the conservative estimate counts input and
//! output as up to 2 MP each, $0.048 per edit. The price on any account is
//! not proven.
//! Metering (authorize, edit, charge) lives in `ImageService`.

use crate::fal_image::{
    DataUri, FalError, FalQueue, IMAGE_OUTPUT_FORMAT, ImageAsset, ImageJob, ImageSize,
    PROVIDER_NAME, image_source_mime, invalid_input,
};
use async_trait::async_trait;
use base64::Engine as _;
use clovy_config::UpstreamConfig;
use clovy_domain::{DomainError, GeneratedImage, ImageEditRequest, ImageEditor};
use image::{
    DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Rgba, RgbaImage,
    imageops::FilterType, metadata::Orientation,
};
use serde::Serialize;
use std::{
    io::{self, BufRead, BufReader, Cursor, Read, Seek, SeekFrom},
    time::Duration,
};

/// Curated edit endpoints, keyed by the model id Clovy prices.
const EDIT_ENDPOINTS: [&str; 1] = ["fal-ai/flux-2/edit"];
/// Requested output sides stay within the schema's `image_size` limits.
const MIN_SIDE: u32 = 512;
const MAX_SIDE: u32 = 2048;
/// Output pixel budget: the 1024x1024 output of the model page's price example.
const PIXEL_BUDGET: u64 = 1024 * 1024;
/// Neutral fill around a source drawn onto a canvas.
const PADDING: Rgba<u8> = Rgba([128, 128, 128, 255]);
const SOURCE_UNSUPPORTED: &str = "image_edit_source_unsupported";
const SOURCE_TOO_LARGE: &str = "image_edit_source_too_large";

/// `ImageEditor` over fal's queue for the curated edit endpoint.
pub struct FalImageEditor {
    queue: FalQueue,
    /// End-to-end budget for the whole edit leg (see `FalImageGenerator`).
    leg_budget: Duration,
}

impl FalImageEditor {
    pub fn from_config(
        http: reqwest::Client,
        config: &UpstreamConfig,
        leg_budget: Duration,
    ) -> Self {
        Self {
            queue: FalQueue::from_config(http, config, leg_budget),
            leg_budget,
        }
    }
}

#[async_trait]
impl ImageEditor for FalImageEditor {
    async fn edit(&self, request: ImageEditRequest) -> Result<GeneratedImage, DomainError> {
        let prompt = request.prompt.trim();
        if prompt.is_empty() {
            return Err(invalid_input("image_edit_prompt_empty"));
        }
        if request.image_base64.trim().is_empty() {
            return Err(invalid_input("image_edit_source_empty"));
        }
        let Some(endpoint) = EDIT_ENDPOINTS
            .into_iter()
            .find(|endpoint| *endpoint == request.model.0)
        else {
            return Err(invalid_input("image_edit_model_unsupported"));
        };
        if image_source_mime(&request.mime_type).is_none() {
            return Err(invalid_input(SOURCE_UNSUPPORTED));
        }
        let plan = plan_edit(request.image_base64).await?;
        let source = match &plan {
            EditPlan::AsIs {
                mime_type, source, ..
            } => DataUri {
                mime_type,
                base64: source.trim(),
            },
            EditPlan::Reduced { png_base64, .. } | EditPlan::Canvas { png_base64, .. } => DataUri {
                mime_type: "image/png",
                base64: png_base64,
            },
        };
        let input = EditInput {
            prompt,
            image_urls: [source],
            image_size: plan.size(),
            num_images: 1,
            output_format: IMAGE_OUTPUT_FORMAT,
            sync_mode: true,
            enable_safety_checker: request.safe_mode,
        };
        let asset = self
            .queue
            .run_image_job(&ImageJob {
                endpoint,
                input: &input,
                key: self.queue.key_for(&request.provider_credentials),
                budget: self.leg_budget,
            })
            .await
            .map_err(edit_error)?;
        let (size, content) = match plan {
            EditPlan::AsIs { size, .. } | EditPlan::Reduced { size, .. } => (size, None),
            EditPlan::Canvas { size, content, .. } => (size, Some(content)),
        };
        let asset = finish(asset, size, content).await.map_err(edit_error)?;
        Ok(GeneratedImage {
            image_base64: base64::engine::general_purpose::STANDARD.encode(&asset.bytes),
            mime_type: asset.mime_type.to_string(),
            model: request.model.0,
            provider: PROVIDER_NAME.to_string(),
        })
    }
}

/// How one edit's source is sent and its result read back.
enum EditPlan {
    /// The source, within the pixel budget, goes exactly as received (the
    /// request's own text, untrimmed); the result must have `size`.
    AsIs {
        size: ImageSize,
        mime_type: &'static str,
        source: String,
    },
    /// The whole source, over the pixel budget, was scaled to exactly
    /// `size`; the result must have `size`.
    Reduced { size: ImageSize, png_base64: String },
    /// The source was drawn onto a canvas of exactly `size`; the result must
    /// have `size` and is cropped back to `content`.
    Canvas {
        size: ImageSize,
        content: Rect,
        png_base64: String,
    },
}

impl EditPlan {
    fn size(&self) -> ImageSize {
        match self {
            Self::AsIs { size, .. } | Self::Reduced { size, .. } | Self::Canvas { size, .. } => {
                *size
            }
        }
    }
}

/// The part of the requested output that holds the source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// An image's geometry from its header.
struct HeaderView {
    /// Display size: the stored size with a 90/270-degree orientation applied.
    width: u32,
    height: u32,
    format: ImageFormat,
    orientation: Orientation,
}

impl HeaderView {
    /// Whether the source must be redrawn: too elongated for both size limits,
    /// or stored with an orientation the pixels do not carry yet (fal's EXIF
    /// handling is undocumented, so Clovy sends upright pixels).
    fn needs_canvas(&self) -> bool {
        let long = u64::from(self.width.max(self.height));
        let short = u64::from(self.width.min(self.height));
        long > short * u64::from(MAX_SIDE / MIN_SIDE)
            || self.orientation != Orientation::NoTransforms
    }

    /// Pixels as shown.
    fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// Why a header could not be read.
enum HeaderError {
    Unreadable,
    OverLimits,
}

/// Plans one edit on a blocking thread that owns the source text (moved in,
/// never copied whole); see the module's notes on blocking work.
async fn plan_edit(source: String) -> Result<EditPlan, DomainError> {
    tokio::task::spawn_blocking(move || prepare(source))
        .await
        .map_err(|error| {
            tracing::error!(
                panicked = error.is_panic(),
                "fal image edit: source preparation did not finish"
            );
            DomainError::UpstreamProvider
        })?
}

/// Keeps the source when it can go as received; otherwise renders the PNG
/// that goes instead: the whole source at the requested size when it is over
/// the pixel budget, or the canvas.
fn prepare(source: String) -> Result<EditPlan, DomainError> {
    let text = source.trim();
    let view = Base64Bytes::new(text)
        .ok_or(HeaderError::Unreadable)
        .and_then(|bytes| read_header(BufReader::new(bytes)))
        .map_err(|error| match error {
            HeaderError::Unreadable => invalid_input(SOURCE_UNSUPPORTED),
            HeaderError::OverLimits => invalid_input(SOURCE_TOO_LARGE),
        })?;
    let size = output_size(view.width, view.height);
    if view.needs_canvas() {
        let content = content_rect(view.width, view.height, size);
        let png_base64 = render_canvas(text, view.orientation, size, content)?;
        return Ok(EditPlan::Canvas {
            size,
            content,
            png_base64,
        });
    }
    if view.pixels() > PIXEL_BUDGET {
        let png_base64 = reduce(text, size)?;
        return Ok(EditPlan::Reduced { size, png_base64 });
    }
    Ok(EditPlan::AsIs {
        size,
        mime_type: view.format.to_mime_type(),
        source,
    })
}

/// Reads format, stored size and orientation without decoding pixels (a JPEG
/// decoder still reads the whole file first).
fn read_header<R: BufRead + Seek>(image: R) -> Result<HeaderView, HeaderError> {
    let reader = ImageReader::new(image)
        .with_guessed_format()
        .map_err(|_| HeaderError::Unreadable)?;
    let format = reader.format().ok_or(HeaderError::Unreadable)?;
    let mut decoder = reader.into_decoder().map_err(|error| {
        tracing::warn!(
            kind = image_error_kind(&error),
            "fal image edit: image header unreadable"
        );
        if matches!(error, ImageError::Limits(_)) {
            HeaderError::OverLimits
        } else {
            HeaderError::Unreadable
        }
    })?;
    let (stored_width, stored_height) = decoder.dimensions();
    if stored_width == 0 || stored_height == 0 {
        return Err(HeaderError::Unreadable);
    }
    // Unreadable orientation metadata leaves the pixels as stored, which is
    // also what a reader ignoring it would show.
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (width, height) = if swaps_axes(orientation) {
        (stored_height, stored_width)
    } else {
        (stored_width, stored_height)
    };
    Ok(HeaderView {
        width,
        height,
        format,
        orientation,
    })
}

/// The bytes a standard, padded base64 text encodes, readable and seekable
/// without decoding the whole text up front: each read decodes only the
/// 4-character groups it covers, straight into the caller's buffer. A decoder
/// that reads to the end (JPEG) still ends up holding the whole file.
struct Base64Bytes<'a> {
    text: &'a [u8],
    len: usize,
    position: usize,
}

impl<'a> Base64Bytes<'a> {
    /// `None` unless `text` is a non-empty whole number of 4-character groups.
    fn new(text: &'a str) -> Option<Self> {
        let text = text.as_bytes();
        if text.is_empty() || !text.len().is_multiple_of(4) {
            return None;
        }
        let padding = text
            .iter()
            .rev()
            .take(2)
            .take_while(|byte| **byte == b'=')
            .count();
        Some(Self {
            text,
            len: text.len() / 4 * 3 - padding,
            position: 0,
        })
    }
}

impl Read for Base64Bytes<'_> {
    /// Whole groups decode straight into `buf`; a group read only in part (at
    /// either end of the read, or the padded last one) goes through a
    /// three-byte stack buffer. Nothing is allocated.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let wanted = buf.len().min(self.len.saturating_sub(self.position));
        let whole_groups = self.len / 3;
        let mut read = 0;
        while read < wanted {
            let position = self.position + read;
            let (group, skip) = (position / 3, position % 3);
            let rest = &mut buf[read..wanted];
            let whole = (rest.len() / 3).min(whole_groups.saturating_sub(group));
            let count = if skip == 0 && whole > 0 {
                decode_into(
                    &self.text[group * 4..(group + whole) * 4],
                    &mut rest[..whole * 3],
                )?
            } else {
                let mut decoded = [0_u8; 3];
                let decoded_len = decode_into(&self.text[group * 4..group * 4 + 4], &mut decoded)?;
                let count = decoded_len.saturating_sub(skip).min(rest.len());
                rest[..count].copy_from_slice(&decoded[skip..skip + count]);
                count
            };
            if count == 0 {
                break;
            }
            read += count;
        }
        self.position += read;
        Ok(read)
    }
}

fn decode_into(text: &[u8], output: &mut [u8]) -> io::Result<usize> {
    base64::engine::general_purpose::STANDARD
        .decode_slice(text, output)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

impl Seek for Base64Bytes<'_> {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        let outside = || io::Error::new(io::ErrorKind::InvalidInput, "seek outside the source");
        let position = match target {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::End(offset) => {
                i128::try_from(self.len).map_err(|_| outside())? + i128::from(offset)
            }
            SeekFrom::Current(offset) => {
                i128::try_from(self.position).map_err(|_| outside())? + i128::from(offset)
            }
        };
        self.position = usize::try_from(position).map_err(|_| outside())?;
        u64::try_from(self.position).map_err(|_| outside())
    }
}

fn swaps_axes(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// The output size for a source of `width` x `height` (display orientation).
/// With `long:short` the source ratio clamped to `MAX_SIDE / MIN_SIDE`, the
/// short side is `floor(sqrt(budget * short / long))` and the long side that
/// ratio applied to it, to the nearest pixel; when that rounding passes the
/// budget, the short side gives up one pixel. Both sides land in 512..=2048.
fn output_size(width: u32, height: u32) -> ImageSize {
    let max_ratio = u64::from(MAX_SIDE / MIN_SIDE);
    let source_long = u64::from(width.max(height));
    let source_short = u64::from(width.min(height));
    let (long, short) = if source_long > source_short * max_ratio {
        (max_ratio, 1)
    } else {
        (source_long, source_short)
    };
    let mut out_short = (PIXEL_BUDGET * short / long).isqrt();
    let mut out_long = nearest_pixel(out_short * long, short);
    while out_short * out_long > PIXEL_BUDGET {
        out_short -= 1;
        out_long = nearest_pixel(out_short * long, short);
    }
    let (out_long, out_short) = (side(out_long), side(out_short));
    if width >= height {
        ImageSize {
            width: out_long,
            height: out_short,
        }
    } else {
        ImageSize {
            width: out_short,
            height: out_long,
        }
    }
}

/// `numerator / denominator` rounded to the nearest whole pixel.
fn nearest_pixel(numerator: u64, denominator: u64) -> u64 {
    (2 * numerator + denominator) / (2 * denominator)
}

/// The centered rectangle a `width` x `height` source fills when scaled,
/// without stretching, to fit inside `size`.
fn content_rect(width: u32, height: u32, size: ImageSize) -> Rect {
    let (source_width, source_height) = (u64::from(width), u64::from(height));
    let (out_width, out_height) = (u64::from(size.width), u64::from(size.height));
    let (fit_width, fit_height) = if source_width * out_height >= source_height * out_width {
        let fit_height = nearest_pixel(source_height * out_width, source_width);
        (out_width, fit_height.clamp(1, out_height))
    } else {
        let fit_width = nearest_pixel(source_width * out_height, source_height);
        (fit_width.clamp(1, out_width), out_height)
    };
    Rect {
        x: side((out_width - fit_width) / 2),
        y: side((out_height - fit_height) / 2),
        width: side(fit_width),
        height: side(fit_height),
    }
}

/// Sides here never exceed `MAX_SIDE`.
fn side(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(MAX_SIDE)
}

/// Decodes the whole source from its base64 text, within the decoder's
/// default limits.
fn decode_source(text: &str) -> Result<DynamicImage, DomainError> {
    let bytes = Base64Bytes::new(text).ok_or_else(|| invalid_input(SOURCE_UNSUPPORTED))?;
    ImageReader::new(BufReader::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid_input(SOURCE_UNSUPPORTED))?
        .decode()
        .map_err(|error| {
            tracing::warn!(
                kind = image_error_kind(&error),
                "fal image edit: source image unreadable"
            );
            if matches!(error, ImageError::Limits(_)) {
                invalid_input(SOURCE_TOO_LARGE)
            } else {
                invalid_input(SOURCE_UNSUPPORTED)
            }
        })
}

/// Scales the whole upright source to exactly `size`, returned as base64
/// PNG: nothing is cut or padded.
fn reduce(text: &str, size: ImageSize) -> Result<String, DomainError> {
    let reduced = decode_source(text)?.resize_exact(size.width, size.height, FilterType::Lanczos3);
    png_base64(&reduced)
}

/// Decodes the source, turns it upright, scales it into `content` and draws
/// it on a neutral canvas of exactly `size`, returned as base64 PNG.
fn render_canvas(
    text: &str,
    orientation: Orientation,
    size: ImageSize,
    content: Rect,
) -> Result<String, DomainError> {
    let mut source = decode_source(text)?;
    source.apply_orientation(orientation);
    let fitted = source
        .resize_exact(content.width, content.height, FilterType::Lanczos3)
        .into_rgba8();
    let mut canvas = RgbaImage::from_pixel(size.width, size.height, PADDING);
    image::imageops::replace(
        &mut canvas,
        &fitted,
        i64::from(content.x),
        i64::from(content.y),
    );
    png_base64(&DynamicImage::ImageRgba8(canvas))
}

/// A prepared source as base64 PNG, encoded once.
fn png_base64(image: &DynamicImage) -> Result<String, DomainError> {
    encode_png(image)
        .map(|png| base64::engine::general_purpose::STANDARD.encode(png))
        .map_err(|error| {
            tracing::error!(
                kind = image_error_kind(&error),
                "fal image edit: prepared source could not be encoded"
            );
            DomainError::UpstreamProvider
        })
}

/// The image error's category, for logs: decoder messages can describe the
/// user's image, so only the kind is recorded.
fn image_error_kind(error: &ImageError) -> &'static str {
    if matches!(error, ImageError::Limits(_)) {
        "limits"
    } else if matches!(error, ImageError::Unsupported(_)) {
        "unsupported"
    } else if matches!(error, ImageError::Decoding(_)) {
        "decoding"
    } else if matches!(error, ImageError::Encoding(_)) {
        "encoding"
    } else if matches!(error, ImageError::IoError(_)) {
        "io"
    } else {
        "other"
    }
}

/// A result for a source sent whole must report the requested size in its
/// header; any other size is not the edit asked for.
fn require_size(asset: ImageAsset, size: ImageSize) -> Result<ImageAsset, FalError> {
    let Ok(view) = read_header(Cursor::new(&asset.bytes)) else {
        return Err(FalError::MediaUnavailable);
    };
    if view.width != size.width || view.height != size.height {
        tracing::error!(
            requested_width = size.width,
            requested_height = size.height,
            width = view.width,
            height = view.height,
            "fal image edit: output size differs from the requested size"
        );
        return Err(FalError::MediaUnavailable);
    }
    Ok(asset)
}

/// Checks the result on a blocking thread that owns its bytes: it must have
/// exactly `size`, and a canvas edit is then cropped back to `content`.
async fn finish(
    asset: ImageAsset,
    size: ImageSize,
    content: Option<Rect>,
) -> Result<ImageAsset, FalError> {
    tokio::task::spawn_blocking(move || match content {
        None => require_size(asset, size),
        Some(content) => crop_result(&asset.bytes, size, content),
    })
    .await
    .map_err(|error| {
        tracing::error!(
            panicked = error.is_panic(),
            "fal image edit: result check did not finish"
        );
        FalError::Upstream { retryable: false }
    })?
}

/// The output must have exactly the requested size: the crop rectangle comes
/// from that geometry, never from a guess about a different one.
fn crop_result(bytes: &[u8], size: ImageSize, content: Rect) -> Result<ImageAsset, FalError> {
    let edited = image::load_from_memory(bytes).map_err(|_| FalError::MediaUnavailable)?;
    if edited.width() != size.width || edited.height() != size.height {
        tracing::error!(
            requested_width = size.width,
            requested_height = size.height,
            width = edited.width(),
            height = edited.height(),
            "fal image edit: output size differs from the requested size"
        );
        return Err(FalError::MediaUnavailable);
    }
    let cropped = edited.crop_imm(content.x, content.y, content.width, content.height);
    let png = encode_png(&cropped).map_err(|error| {
        tracing::error!(
            kind = image_error_kind(&error),
            "fal image edit: cropped result could not be encoded"
        );
        FalError::Upstream { retryable: false }
    })?;
    Ok(ImageAsset {
        bytes: png,
        mime_type: "image/png",
    })
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, ImageError> {
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, ImageFormat::Png)?;
    Ok(png.into_inner())
}

fn edit_error(error: FalError) -> DomainError {
    match error {
        FalError::Rejected => invalid_input("image_edit_rejected"),
        FalError::ContentPolicy => invalid_input("image_content_violation"),
        FalError::PayloadTooLarge => invalid_input(SOURCE_TOO_LARGE),
        FalError::UserKeyRejected => invalid_input("fal_api_key_rejected"),
        FalError::TooLarge => invalid_input("image_too_large"),
        FalError::NotFound
        | FalError::RequestInaccessible
        | FalError::Busy
        | FalError::Upstream { .. }
        | FalError::MediaUnavailable
        | FalError::GenerationFailed => DomainError::UpstreamProvider,
    }
}

#[derive(Serialize)]
struct EditInput<'a> {
    prompt: &'a str,
    image_urls: [DataUri<'a>; 1],
    image_size: ImageSize,
    num_images: u8,
    output_format: &'static str,
    /// The edited image comes back inline as a data URI.
    sync_mode: bool,
    /// `None` keeps the model default (on); see `FalImageGenerator`.
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_safety_checker: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::{Base64Bytes, FalImageEditor, PADDING, content_rect, output_size};
    use crate::{
        fal_image::{FalQueue, ImageSize, queue_server},
        http,
    };
    use base64::Engine as _;
    use clovy_config::UpstreamConfig;
    use clovy_domain::{
        DomainError, GeneratedImage, ImageEditRequest, ImageEditor, ModelId, ProviderCredentials,
    };
    use image::{DynamicImage, GenericImageView, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{
        io::{Cursor, Read, Seek, SeekFrom},
        time::Duration,
    };
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, method, path},
    };

    const SUBMIT: &str = "/fal-ai/flux-2/edit";
    const STATUS: &str = "/fal-ai/flux-2/requests/req-1/status";
    const RESULT: &str = "/fal-ai/flux-2/requests/req-1";
    const SOURCE_COLOR: Rgba<u8> = Rgba([10, 200, 30, 255]);
    /// The borders of [`framed`] and its center, each unlike the others.
    const TOP: Rgb<u8> = Rgb([230, 30, 30]);
    const RIGHT: Rgb<u8> = Rgb([30, 230, 230]);
    const BOTTOM: Rgb<u8> = Rgb([30, 30, 230]);
    const LEFT: Rgb<u8> = Rgb([230, 230, 30]);
    const CENTER: Rgb<u8> = Rgb([30, 200, 30]);
    /// [`framed`] as stored: top, right, bottom, left.
    const FRAME: [Rgb<u8>; 4] = [TOP, RIGHT, BOTTOM, LEFT];
    /// What the fake edit returns, unlike any source color.
    const EDITED: Rgba<u8> = Rgba([90, 90, 90, 255]);

    fn editor(server: &MockServer) -> FalImageEditor {
        let config = UpstreamConfig {
            api_key: "service_key".to_string(),
            base_url: server.uri(),
        };
        let leg_budget = Duration::from_secs(5);
        FalImageEditor {
            queue: FalQueue::from_config(http::default_client(), &config, leg_budget)
                .with_media_origin(&server.uri()),
            leg_budget,
        }
    }

    fn png(width: u32, height: u32, color: Rgba<u8>) -> String {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, color))
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("test image encodes");
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    }

    /// A `width` x `height` picture with a band of its own color along each
    /// border, a twentieth of the short side thick, around a center of a fifth
    /// color: a copy that lost an edge, or only kept a central part, misses a
    /// band.
    fn framed(width: u32, height: u32) -> RgbImage {
        let band = (width.min(height) / 20).max(1);
        RgbImage::from_fn(width, height, |x, y| {
            if y < band {
                TOP
            } else if y >= height - band {
                BOTTOM
            } else if x < band {
                LEFT
            } else if x >= width - band {
                RIGHT
            } else {
                CENTER
            }
        })
    }

    fn encode(image: RgbImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, format)
            .expect("test image encodes");
        bytes.into_inner()
    }

    /// `jpeg` with an EXIF APP1 segment carrying Orientation = 6: shown
    /// upright, the stored left column is on top and the stored top row on
    /// the right.
    fn with_orientation_6(jpeg: &[u8]) -> Vec<u8> {
        // APP1 "Exif\0\0", little-endian TIFF header, one IFD0 entry:
        // tag 0x0112 (Orientation), type SHORT, count 1, value 6.
        let app1: [u8; 36] = [
            0xFF, 0xE1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0x00, 0x00, b'I', b'I', 0x2A, 0x00,
            0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00,
            0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut with_exif = Vec::with_capacity(jpeg.len() + app1.len());
        with_exif.extend_from_slice(&jpeg[..2]);
        with_exif.extend_from_slice(&app1);
        with_exif.extend_from_slice(&jpeg[2..]);
        with_exif
    }

    /// Whether `image` shows `[top, right, bottom, left]` in the middle of
    /// those borders and [`CENTER`] in its center, within JPEG's error.
    fn shows_frame(image: &DynamicImage, [top, right, bottom, left]: [Rgb<u8>; 4]) -> bool {
        let (width, height) = image.dimensions();
        let near = |x: u32, y: u32, color: Rgb<u8>| {
            let pixel = image.get_pixel(x, y).0;
            pixel
                .into_iter()
                .zip(color.0)
                .all(|(got, want)| got.abs_diff(want) <= 48)
        };
        near(width / 2, 1, top)
            && near(width - 2, height / 2, right)
            && near(width / 2, height - 2, bottom)
            && near(1, height / 2, left)
            && near(width / 2, height / 2, CENTER)
    }

    fn decode(base64: &str) -> DynamicImage {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(base64)
            .expect("valid base64");
        image::load_from_memory(&bytes).expect("valid image")
    }

    fn request(source_base64: String) -> ImageEditRequest {
        ImageEditRequest {
            image_base64: source_base64,
            mime_type: "image/png".to_string(),
            prompt: "make it fluffier".to_string(),
            model: ModelId("fal-ai/flux-2/edit".to_string()),
            safe_mode: Some(false),
            provider_credentials: ProviderCredentials::default(),
        }
    }

    fn invalid(reason: &str) -> DomainError {
        DomainError::InvalidInput {
            reason: reason.to_string(),
        }
    }

    async fn mount_queue(server: &MockServer, size: ImageSize, output_base64: &str) {
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .and(body_partial_json(json!({
                "image_size": { "width": size.width, "height": size.height }
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "request_id": "req-1" })),
            )
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "status": "COMPLETED" })),
            )
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(RESULT))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "images": [{ "url": format!("data:image/png;base64,{output_base64}") }],
                "has_nsfw_concepts": [false],
            })))
            .mount(server)
            .await;
    }

    /// The source image fal received in the submit body, checked to be in
    /// the format its data URI declares.
    async fn sent_image(server: &MockServer) -> DynamicImage {
        let received = server.received_requests().await.expect("recording is on");
        let submit = received
            .iter()
            .find(|request| request.url.path() == SUBMIT)
            .expect("a submit was sent");
        let body: serde_json::Value = serde_json::from_slice(&submit.body).expect("JSON body");
        let uri = body["image_urls"][0].as_str().expect("one source");
        let (mime_type, payload) = uri
            .strip_prefix("data:")
            .and_then(|rest| rest.split_once(";base64,"))
            .expect("a base64 data URI");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("valid base64");
        let format = image::guess_format(&bytes).expect("an image");
        assert_eq!(
            format.to_mime_type(),
            mime_type,
            "declared type of the bytes"
        );
        image::load_from_memory_with_format(&bytes, format).expect("valid image")
    }

    /// Edits an upright [`framed`] source in `format` against a fake edit
    /// answering [`EDITED`] at the requested size; returns what fal received,
    /// the requested size and the edit.
    async fn edit_framed(
        width: u32,
        height: u32,
        format: ImageFormat,
    ) -> (DynamicImage, ImageSize, GeneratedImage) {
        let server = queue_server().await;
        let size = output_size(width, height);
        mount_queue(&server, size, &png(size.width, size.height, EDITED)).await;
        let source = encode(framed(width, height), format);

        let image = editor(&server)
            .edit(ImageEditRequest {
                mime_type: format.to_mime_type().to_string(),
                ..request(base64::engine::general_purpose::STANDARD.encode(source))
            })
            .await
            .expect("edit succeeds");

        (sent_image(&server).await, size, image)
    }

    /// `rounded` is `base * rounded_part / base_part` to the nearest pixel.
    fn keeps_ratio(rounded: u32, base: u32, rounded_part: u32, base_part: u32) -> bool {
        let scaled_rounded = u64::from(rounded) * u64::from(base_part);
        let scaled_base = u64::from(base) * u64::from(rounded_part);
        2 * scaled_rounded.abs_diff(scaled_base) <= u64::from(base_part)
    }

    #[test]
    fn output_sizes_follow_the_documented_limits_and_the_source_ratio() {
        for width in (1..=9000).step_by(73) {
            for height in (1..=9000).step_by(79) {
                let size = output_size(width, height);
                let case = format!("{width}x{height} -> {}x{}", size.width, size.height);
                assert!((512..=2048).contains(&size.width), "{case}");
                assert!((512..=2048).contains(&size.height), "{case}");
                assert!(
                    u64::from(size.width) * u64::from(size.height) <= 1024 * 1024,
                    "{case}"
                );
                // Orientation never flips (a near-square source may round to square).
                assert!(width >= height || size.width <= size.height, "{case}");
                assert!(width <= height || size.width >= size.height, "{case}");
                let (long, short) = (width.max(height), width.min(height));
                let (ratio_long, ratio_short) = if long > short * 4 {
                    (4, 1)
                } else {
                    (long, short)
                };
                let out_long = size.width.max(size.height);
                let out_short = size.width.min(size.height);
                assert!(
                    keeps_ratio(out_long, out_short, ratio_long, ratio_short),
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn a_source_fills_a_centered_rectangle_without_stretching() {
        for (width, height) in [
            (5000, 1000),
            (1000, 5000),
            (4001, 1000),
            (20, 60),
            (1600, 900),
        ] {
            let size = output_size(width, height);
            let rect = content_rect(width, height, size);
            let case = format!("{width}x{height} in {}x{}", size.width, size.height);
            assert!(rect.x + rect.width <= size.width, "{case}");
            assert!(rect.y + rect.height <= size.height, "{case}");
            let right = size.width - rect.x - rect.width;
            let bottom = size.height - rect.y - rect.height;
            assert!(
                rect.x.abs_diff(right) <= 1 && rect.y.abs_diff(bottom) <= 1,
                "{case}"
            );
            if rect.width == size.width {
                assert!(
                    keeps_ratio(rect.height, rect.width, height, width),
                    "{case}"
                );
            } else {
                assert_eq!(rect.height, size.height, "{case}");
                assert!(
                    keeps_ratio(rect.width, rect.height, width, height),
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn base64_bytes_read_and_seek_like_the_decoded_source() {
        for len in [997_usize, 998, 999] {
            let source: Vec<u8> = (0..len)
                .map(|index| u8::try_from(index % 251).expect("below 256"))
                .collect();
            let text = base64::engine::general_purpose::STANDARD.encode(&source);
            let mut reader = Base64Bytes::new(&text).expect("whole groups");
            for (start, take) in [(0, 8), (1, 5), (2, 300), (500, 7), (len - 3, 10)] {
                reader
                    .seek(SeekFrom::Start(u64::try_from(start).expect("fits")))
                    .expect("seeks");
                let mut buffer = vec![0; take];
                let read = reader.read(&mut buffer).expect("reads");
                let end = (start + take).min(len);
                assert_eq!(&buffer[..read], &source[start..end], "{len} {start} {take}");
            }
            assert_eq!(
                reader.seek(SeekFrom::End(0)).expect("seeks"),
                u64::try_from(len).expect("fits")
            );
        }
        assert!(Base64Bytes::new("abc").is_none());
    }

    #[tokio::test]
    async fn a_source_within_the_pixel_budget_is_sent_at_its_own_size() {
        for (width, height, format) in [
            (160, 90, ImageFormat::Png),
            (640, 480, ImageFormat::Jpeg),
            (1024, 1024, ImageFormat::Png),
        ] {
            let case = format!("{width}x{height} {format:?}");

            let (sent, size, image) = edit_framed(width, height, format).await;

            assert_eq!(sent.dimensions(), (width, height), "{case}");
            assert!(shows_frame(&sent, FRAME), "{case}");
            let edited = decode(&image.image_base64);
            assert_eq!(edited.dimensions(), (size.width, size.height), "{case}");
            assert_eq!(
                edited.get_pixel(size.width / 2, size.height / 2),
                EDITED,
                "{case}"
            );
            assert_eq!(image.mime_type, "image/png", "{case}");
            assert_eq!(image.model, "fal-ai/flux-2/edit", "{case}");
            assert_eq!(image.provider, "fal", "{case}");
        }
    }

    #[tokio::test]
    async fn a_source_over_the_pixel_budget_is_sent_whole_at_the_requested_size() {
        for (width, height, format) in [
            (1025, 1024, ImageFormat::Png),
            (1600, 1200, ImageFormat::Png),
            (1200, 1600, ImageFormat::Jpeg),
        ] {
            let case = format!("{width}x{height} {format:?}");

            let (sent, size, image) = edit_framed(width, height, format).await;

            let (sent_width, sent_height) = sent.dimensions();
            assert_eq!(
                (sent_width, sent_height),
                (size.width, size.height),
                "{case}"
            );
            assert!(
                u64::from(sent_width) * u64::from(sent_height) <= 1024 * 1024,
                "{case}"
            );
            let (long, short) = (width.max(height), width.min(height));
            let (out_long, out_short) = (sent_width.max(sent_height), sent_width.min(sent_height));
            assert!(keeps_ratio(out_long, out_short, long, short), "{case}");
            assert!(shows_frame(&sent, FRAME), "{case}");
            let edited = decode(&image.image_base64);
            assert_eq!(edited.dimensions(), (size.width, size.height), "{case}");
        }
    }

    #[tokio::test]
    async fn an_elongated_source_is_padded_then_cropped_back_to_its_proportion() {
        let server = queue_server().await;
        let size = output_size(1000, 200);
        mount_queue(
            &server,
            size,
            &png(size.width, size.height, Rgba([200, 20, 20, 255])),
        )
        .await;

        let image = editor(&server)
            .edit(request(png(1000, 200, SOURCE_COLOR)))
            .await
            .expect("edit succeeds");

        let canvas = sent_image(&server).await;
        assert_eq!(canvas.dimensions(), (size.width, size.height));
        assert_eq!(canvas.get_pixel(0, 0), PADDING);
        let center = canvas.get_pixel(size.width / 2, size.height / 2).0;
        assert!(
            center[0] < 40 && center[1] > 170,
            "source at the center: {center:?}"
        );
        let (width, height) = decode(&image.image_base64).dimensions();
        assert_eq!(width, size.width);
        assert!(keeps_ratio(height, width, 1, 5), "{width}x{height}");
        assert_eq!(image.mime_type, "image/png");
    }

    #[tokio::test]
    async fn an_exif_rotated_photo_is_sent_and_returned_upright() {
        let server = queue_server().await;
        // Stored 1600x1200, over the pixel budget; upright it is 1200x1600.
        let size = output_size(1200, 1600);
        mount_queue(&server, size, &png(size.width, size.height, EDITED)).await;
        let jpeg = with_orientation_6(&encode(framed(1600, 1200), ImageFormat::Jpeg));

        let image = editor(&server)
            .edit(ImageEditRequest {
                mime_type: "image/jpeg".to_string(),
                ..request(base64::engine::general_purpose::STANDARD.encode(jpeg))
            })
            .await
            .expect("edit succeeds");

        assert!(size.width < size.height, "upright size is portrait");
        let sent = sent_image(&server).await;
        assert_eq!(sent.dimensions(), (size.width, size.height));
        assert!(
            shows_frame(&sent, [LEFT, TOP, RIGHT, BOTTOM]),
            "stored left column on top"
        );
        let (width, height) = decode(&image.image_base64).dimensions();
        assert!(keeps_ratio(height, width, 4, 3), "{width}x{height}");
    }

    #[tokio::test]
    async fn a_result_at_another_size_is_not_returned_as_the_edit() {
        // One source per path: sent as received, reduced, drawn on a canvas.
        for (width, height) in [(160, 90), (1600, 1200), (1000, 200)] {
            let server = queue_server().await;
            mount_queue(
                &server,
                output_size(width, height),
                &png(1024, 1024, SOURCE_COLOR),
            )
            .await;

            let error = editor(&server)
                .edit(request(png(width, height, SOURCE_COLOR)))
                .await
                .expect_err("no square or reinterpreted result");

            assert_eq!(error, DomainError::UpstreamProvider, "{width}x{height}");
        }
    }

    #[tokio::test]
    async fn a_source_over_fals_file_limit_is_reported_as_such() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .respond_with(ResponseTemplate::new(422).set_body_json(json!({
                "detail": [{
                    "loc": ["body", "image_urls", 0],
                    "msg": "File too large",
                    "type": "file_too_large",
                    "ctx": { "max_size": 10_485_760 },
                }]
            })))
            .mount(&server)
            .await;

        let error = editor(&server)
            .edit(request(png(64, 48, SOURCE_COLOR)))
            .await
            .expect_err("fal refuses the source");

        assert_eq!(error, invalid("image_edit_source_too_large"));
    }

    #[tokio::test]
    async fn requests_are_checked_before_calling_fal() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let editor = editor(&server);
        let valid = || request(png(64, 48, SOURCE_COLOR));
        let not_an_image =
            base64::engine::general_purpose::STANDARD.encode(b"<html>not an image</html>");

        let html = editor
            .edit(ImageEditRequest {
                mime_type: "text/html".to_string(),
                ..valid()
            })
            .await;
        let injected = editor
            .edit(ImageEditRequest {
                mime_type: "image/png;base64,AAAA".to_string(),
                ..valid()
            })
            .await;
        let unreadable = editor.edit(request(not_an_image)).await;
        let model = editor
            .edit(ImageEditRequest {
                model: ModelId("firered-image-edit".to_string()),
                ..valid()
            })
            .await;
        let prompt = editor
            .edit(ImageEditRequest {
                prompt: "   ".to_string(),
                ..valid()
            })
            .await;

        assert_eq!(html, Err(invalid("image_edit_source_unsupported")));
        assert_eq!(injected, Err(invalid("image_edit_source_unsupported")));
        assert_eq!(unreadable, Err(invalid("image_edit_source_unsupported")));
        assert_eq!(model, Err(invalid("image_edit_model_unsupported")));
        assert_eq!(prompt, Err(invalid("image_edit_prompt_empty")));
    }
}
