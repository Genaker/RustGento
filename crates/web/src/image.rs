//! `GET /image/webp` -- fetch a source image (local file or remote URL),
//! resize it, and serve it re-encoded, matching Go's `html/image.go` query
//! contract (`src`, `w`, `h`, `type`, `q`) closely enough for the product/
//! category page templates to use unchanged.

use axum::extract::Query;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use image::{DynamicImage, ImageFormat};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct ImageQuery {
    src: String,
    w: Option<u32>,
    h: Option<u32>,
    #[serde(rename = "type")]
    kind: Option<String>,
    q: Option<u8>,
}

const CACHE_DIR: &str = "var/cache/image_cache";

pub async fn show(Query(q): Query<ImageQuery>) -> Response {
    if q.src.is_empty() {
        return (StatusCode::BAD_REQUEST, "src parameter is required").into_response();
    }
    let width = q.w.unwrap_or(0);
    let height = q.h.unwrap_or(0);
    let kind = q.kind.unwrap_or_else(|| "jpeg".to_string());
    let quality = q.q.filter(|q| (1..=100).contains(q)).unwrap_or(90);

    let (content_type, ext, format) = match kind.as_str() {
        "png" => ("image/png", "png", ImageFormat::Png),
        "webp" => ("image/webp", "webp", ImageFormat::WebP),
        _ => ("image/jpeg", "jpg", ImageFormat::Jpeg),
    };

    let cache_key = {
        let mut hasher = Sha256::new();
        hasher.update(format!("{}_{width}_{height}_{kind}_{quality}", q.src));
        format!("{:x}", hasher.finalize())
    };
    let cache_path = PathBuf::from(CACHE_DIR).join(format!("{cache_key}.{ext}"));

    if let Ok(bytes) = tokio::fs::read(&cache_path).await {
        return cached_response(content_type, bytes);
    }

    let source_bytes = match fetch_source(&q.src).await {
        Ok(bytes) => bytes,
        Err(err) => return err.into_response(),
    };
    let img = match image::load_from_memory(&source_bytes) {
        Ok(img) => img,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("failed to decode image: {e}")).into_response(),
    };

    let resized = resize(img, width, height);

    let mut out = Vec::new();
    if let Err(e) = resized.write_to(&mut std::io::Cursor::new(&mut out), format) {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("failed to encode image: {e}")).into_response();
    }
    // Best-effort: a warm cache saves the next request a resize/encode
    // round trip, but a write failure (e.g. read-only filesystem) shouldn't
    // fail the request that's already produced good bytes to serve.
    if tokio::fs::create_dir_all(CACHE_DIR).await.is_ok() {
        let _ = tokio::fs::write(&cache_path, &out).await;
    }

    cached_response(content_type, out)
}

fn cached_response(content_type: &'static str, bytes: Vec<u8>) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], bytes).into_response()
}

async fn fetch_source(src: &str) -> Result<Vec<u8>, (StatusCode, &'static str)> {
    if src.starts_with("http://") || src.starts_with("https://") {
        let resp = reqwest::get(src).await.map_err(|_| (StatusCode::NOT_FOUND, "failed to fetch remote image"))?;
        resp.bytes().await.map(|b| b.to_vec()).map_err(|_| (StatusCode::BAD_GATEWAY, "failed to read remote image body"))
    } else {
        tokio::fs::read(src).await.map_err(|_| (StatusCode::NOT_FOUND, "image not found"))
    }
}

/// Resizes to fit within `width`x`height`, matching Go's behavior:
/// - both given: scale to fit inside the box, then pad onto a white
///   background of exactly `width`x`height` (letterboxed, not cropped)
/// - only one given: scale that dimension, preserving aspect ratio
/// - neither given: return unchanged
fn resize(img: DynamicImage, width: u32, height: u32) -> DynamicImage {
    if width == 0 && height == 0 {
        return img;
    }
    if width > 0 && height > 0 {
        let resized = img.resize(width, height, image::imageops::FilterType::CatmullRom);
        let mut canvas = DynamicImage::new_rgba8(width, height);
        for px in canvas.as_mut_rgba8().unwrap().pixels_mut() {
            *px = image::Rgba([255, 255, 255, 255]);
        }
        let x = (width.saturating_sub(resized.width())) / 2;
        let y = (height.saturating_sub(resized.height())) / 2;
        image::imageops::overlay(&mut canvas, &resized, x as i64, y as i64);
        canvas
    } else if width > 0 {
        img.resize(width, u32::MAX, image::imageops::FilterType::CatmullRom)
    } else {
        img.resize(u32::MAX, height, image::imageops::FilterType::CatmullRom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    fn solid(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255])))
    }

    #[test]
    fn no_dimensions_returns_unchanged() {
        let img = solid(100, 50);
        let out = resize(img.clone(), 0, 0);
        assert_eq!(out.dimensions(), img.dimensions());
    }

    #[test]
    fn width_only_preserves_aspect_ratio() {
        let out = resize(solid(200, 100), 100, 0);
        assert_eq!(out.width(), 100);
        assert_eq!(out.height(), 50);
    }

    #[test]
    fn height_only_preserves_aspect_ratio() {
        let out = resize(solid(200, 100), 0, 50);
        assert_eq!(out.height(), 50);
        assert_eq!(out.width(), 100);
    }

    #[test]
    fn both_dimensions_produce_exact_canvas_size() {
        // A wide source image fit into a square box should be letterboxed
        // (padded), not stretched or cropped -- output dims must match the
        // requested box exactly regardless of source aspect ratio.
        let out = resize(solid(400, 100), 200, 200);
        assert_eq!(out.dimensions(), (200, 200));
    }

    #[test]
    fn square_box_pads_a_tall_source_too() {
        let out = resize(solid(100, 400), 200, 200);
        assert_eq!(out.dimensions(), (200, 200));
    }
}
