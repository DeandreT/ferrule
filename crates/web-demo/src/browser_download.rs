use std::fmt;

#[cfg(target_arch = "wasm32")]
const UTF8_TEXT_MIME: &str = "text/plain;charset=utf-8";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserDownloadError {
    EmptyFilename,
    #[cfg(not(target_arch = "wasm32"))]
    UnsupportedPlatform,
    #[cfg(target_arch = "wasm32")]
    BrowserUnavailable(&'static str),
    #[cfg(target_arch = "wasm32")]
    BrowserOperation {
        operation: &'static str,
        detail: Option<String>,
    },
}

impl fmt::Display for BrowserDownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFilename => formatter.write_str("download filename cannot be empty"),
            #[cfg(not(target_arch = "wasm32"))]
            Self::UnsupportedPlatform => {
                formatter.write_str("browser downloads are unavailable on this platform")
            }
            #[cfg(target_arch = "wasm32")]
            Self::BrowserUnavailable(api) => write!(formatter, "browser `{api}` is unavailable"),
            #[cfg(target_arch = "wasm32")]
            Self::BrowserOperation {
                operation,
                detail: Some(detail),
            } => write!(formatter, "browser failed to {operation}: {detail}"),
            #[cfg(target_arch = "wasm32")]
            Self::BrowserOperation {
                operation,
                detail: None,
            } => write!(formatter, "browser failed to {operation}"),
        }
    }
}

impl std::error::Error for BrowserDownloadError {}

/// Starts a browser download containing UTF-8 text.
///
/// This returns after dispatching the anchor click; the browser owns the
/// resulting download. Native builds return an unsupported-platform error.
pub fn download_utf8_text(filename: &str, text: &str) -> Result<(), BrowserDownloadError> {
    let filename = validate_filename(filename)?;
    download_utf8_text_impl(filename, text)
}

fn validate_filename(filename: &str) -> Result<&str, BrowserDownloadError> {
    if filename.trim().is_empty() {
        return Err(BrowserDownloadError::EmptyFilename);
    }
    Ok(filename)
}

#[cfg(not(target_arch = "wasm32"))]
fn download_utf8_text_impl(_filename: &str, _text: &str) -> Result<(), BrowserDownloadError> {
    Err(BrowserDownloadError::UnsupportedPlatform)
}

#[cfg(target_arch = "wasm32")]
fn download_utf8_text_impl(filename: &str, text: &str) -> Result<(), BrowserDownloadError> {
    use eframe::wasm_bindgen::{JsCast as _, JsValue};
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(text));

    let options = BlobPropertyBag::new();
    options.set_type(UTF8_TEXT_MIME);
    let blob = Blob::new_with_str_sequence_and_options(&parts, &options)
        .map_err(|error| browser_operation("create the download data", error))?;
    let object_url = Url::create_object_url_with_blob(&blob)
        .map_err(|error| browser_operation("create the download URL", error))?;

    let result = (|| {
        let window = web_sys::window().ok_or(BrowserDownloadError::BrowserUnavailable("window"))?;
        let document = window
            .document()
            .ok_or(BrowserDownloadError::BrowserUnavailable("document"))?;
        let body = document
            .body()
            .ok_or(BrowserDownloadError::BrowserUnavailable("document.body"))?;
        let anchor = document
            .create_element("a")
            .map_err(|error| browser_operation("create the download link", error))?
            .dyn_into::<HtmlAnchorElement>()
            .map_err(|error| browser_operation("prepare the download link", error.into()))?;

        anchor.set_href(&object_url);
        anchor.set_download(filename);
        body.append_child(&anchor)
            .map_err(|error| browser_operation("attach the download link", error))?;
        anchor.click();
        anchor.remove();
        Ok(())
    })();

    let revoke_result = Url::revoke_object_url(&object_url)
        .map_err(|error| browser_operation("release the download URL", error));
    result.and(revoke_result)
}

#[cfg(target_arch = "wasm32")]
fn browser_operation(
    operation: &'static str,
    error: eframe::wasm_bindgen::JsValue,
) -> BrowserDownloadError {
    BrowserDownloadError::BrowserOperation {
        operation,
        detail: error.as_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_validation_rejects_empty_text() {
        assert_eq!(
            validate_filename(" \t\n"),
            Err(BrowserDownloadError::EmptyFilename)
        );
    }

    #[test]
    fn filename_validation_preserves_a_valid_name() {
        assert_eq!(
            validate_filename("mapped output.xml"),
            Ok("mapped output.xml")
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_download_fallback_is_harmless() {
        assert_eq!(
            download_utf8_text("output.xml", "<output />"),
            Err(BrowserDownloadError::UnsupportedPlatform)
        );
    }
}
