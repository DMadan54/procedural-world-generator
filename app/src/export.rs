//! Exporting: the map as a PNG and the world as JSON. Native builds ask
//! where to save with a file dialog; the web build hands the browser a
//! download.

use eframe::egui::ColorImage;

/// Encodes RGB pixels as a PNG.
pub fn png(width: usize, height: usize, rgb: &[u8]) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(
            rgb,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Scales a debug image up by a whole factor, keeping cells crisp.
pub fn upscale(image: &ColorImage, factor: usize) -> (usize, usize, Vec<u8>) {
    let [w, h] = image.size;
    let (ow, oh) = (w * factor, h * factor);
    let mut rgb = Vec::with_capacity(ow * oh * 3);
    for y in 0..oh {
        for x in 0..ow {
            let c = image.pixels[(y / factor) * w + x / factor];
            rgb.extend_from_slice(&[c.r(), c.g(), c.b()]);
        }
    }
    (ow, oh, rgb)
}

/// Saves `bytes` as `name`: a save dialog on native (returns false if the
/// user cancels), a browser download on the web.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(name: &str, kind: &str, extension: &str, bytes: &[u8]) -> Result<bool, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_file_name(name)
        .add_filter(kind, &[extension])
        .save_file()
    else {
        return Ok(false);
    };
    std::fs::write(&path, bytes).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    Ok(true)
}

#[cfg(target_arch = "wasm32")]
pub fn save(name: &str, _kind: &str, extension: &str, bytes: &[u8]) -> Result<bool, String> {
    use wasm_bindgen::JsCast;
    let fail = |what: &str| format!("Couldn't start the download ({what}).");
    let mime = match extension {
        "png" => "image/png",
        "json" => "application/json",
        "md" => "text/markdown",
        "html" => "text/html",
        _ => "application/octet-stream",
    };
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|_| fail("blob"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| fail("url"))?;
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| fail("document"))?;
    let link: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .map_err(|_| fail("link"))?
        .dyn_into()
        .map_err(|_| fail("link"))?;
    link.set_href(&url);
    link.set_download(name);
    link.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_has_the_signature_and_size() {
        let bytes = png(3, 2, &[200; 18]).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR width and height, big-endian.
        assert_eq!(&bytes[16..24], &[0, 0, 0, 3, 0, 0, 0, 2]);
    }

    #[test]
    fn upscale_repeats_each_cell() {
        let image = ColorImage::from_rgb([2, 1], &[10, 20, 30, 40, 50, 60]);
        let (w, h, rgb) = upscale(&image, 2);
        assert_eq!((w, h), (4, 2));
        assert_eq!(
            &rgb[..12],
            &[10, 20, 30, 10, 20, 30, 40, 50, 60, 40, 50, 60]
        );
    }
}
