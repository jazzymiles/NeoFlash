//! Bounded PNG/JPEG decoding, shared between immutable authoring and playback.
use crate::{MAX_BITMAP_DIMENSION, ResourceError, ResourceUnit};
use image::{ImageFormat, ImageReader, Limits};
use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Arc, Mutex, OnceLock, Weak},
};

#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}
struct Cached {
    source: Weak<str>,
    image: Arc<DecodedImage>,
}
const CACHE_BYTES: usize = 64 * 1024 * 1024;
fn cache() -> &'static Mutex<VecDeque<Cached>> {
    static CACHE: OnceLock<Mutex<VecDeque<Cached>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}
impl ResourceUnit {
    pub(crate) fn image_dimensions(&self) -> Result<(u32, u32), ResourceError> {
        let bytes = self.decoded_bytes()?;
        let format =
            image::guess_format(&bytes).map_err(|e| ResourceError(format!("image format: {e}")))?;
        if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
            return Err(ResourceError(
                "only PNG and JPEG images are supported".into(),
            ));
        }
        let (w, h) = ImageReader::with_format(Cursor::new(bytes), format)
            .into_dimensions()
            .map_err(|e| ResourceError(format!("image dimensions: {e}")))?;
        if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
            return Err(ResourceError("image dimensions must be in 1..=4096".into()));
        }
        Ok((w, h))
    }

    pub fn decode_image(&self) -> Result<Arc<DecodedImage>, ResourceError> {
        {
            let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
            cache.retain(|item| item.source.strong_count() > 0);
            if let Some(index) = cache
                .iter()
                .position(|item| item.source.ptr_eq(&Arc::downgrade(&self.data_base64)))
            {
                let item = cache.remove(index).expect("cache index");
                let image = item.image.clone();
                cache.push_back(item);
                return Ok(image);
            }
        }
        let bytes = self.decoded_bytes()?;
        let result = Arc::new(decode_encoded_image(&bytes)?);
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        let mut bytes: usize = cache.iter().map(|item| item.image.rgba.len()).sum();
        while bytes + result.rgba.len() > CACHE_BYTES {
            let Some(item) = cache.pop_front() else { break };
            bytes -= item.image.rgba.len();
        }
        cache.push_back(Cached {
            source: Arc::downgrade(&self.data_base64),
            image: result.clone(),
        });
        Ok(result)
    }
}

/// Decode a portable encoded image without filesystem access or the authoring cache.
pub(crate) fn encoded_image_dimensions(bytes: &[u8]) -> Result<(u32, u32), ResourceError> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err(ResourceError(
            "encoded image must be within 1..=16 MiB".into(),
        ));
    }
    let format =
        image::guess_format(bytes).map_err(|e| ResourceError(format!("image format: {e}")))?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(ResourceError(
            "only PNG and JPEG images are supported".into(),
        ));
    }
    let (w, h) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|e| ResourceError(format!("image dimensions: {e}")))?;
    if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
        return Err(ResourceError("image dimensions must be in 1..=4096".into()));
    }
    Ok((w, h))
}
pub(crate) fn decode_encoded_image(bytes: &[u8]) -> Result<DecodedImage, ResourceError> {
    encoded_image_dimensions(bytes)?;
    let format = image::guess_format(bytes).map_err(|e| ResourceError(e.to_string()))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_BITMAP_DIMENSION);
    limits.max_image_height = Some(MAX_BITMAP_DIMENSION);
    limits.max_alloc = Some(128 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| ResourceError(format!("bounded image decode: {e}")))?
        .into_rgba8();
    Ok(DecodedImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw().into(),
    })
}
