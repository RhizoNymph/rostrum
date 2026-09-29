//! The pairing link as a QR code, rendered here as inline SVG so the page
//! needs no script library and makes no request for it.

use qrcode::{EcLevel, QrCode, render::svg, types::QrError};

/// Dark modules on white, with the quiet zone: phone cameras read that far
/// more reliably than a dark-theme inversion.
pub fn svg(text: &str) -> Result<String, QrError> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)?;
    let image = code
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .dark_color(svg::Color("#0f1115"))
        .light_color(svg::Color("#ffffff"))
        .build();
    // The renderer emits a standalone document; inline in HTML the XML
    // declaration is noise.
    let start = image.find("<svg").unwrap_or(0);
    Ok(image[start..].replacen(
        "<svg ",
        "<svg role=\"img\" aria-label=\"QR code of the pairing link\" ",
        1,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pairing_link_renders_as_an_inline_svg_element() {
        let svg =
            svg("rostrum://pair?v=1&m=desk&h=192.168.0.111,100.64.0.10&p=8485&c=K7QXM2PD&fp=AAAA")
                .expect("renders");
        assert!(svg.starts_with("<svg "), "{}", &svg[..40]);
        assert!(svg.ends_with("</svg>"));
        assert!(!svg.contains("<?xml"));
        assert!(svg.contains("aria-label"));
        assert!(svg.contains("#0f1115"));
    }
}
