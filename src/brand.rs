//! Embedded, original Felifox brand artwork (see `assets/README.md`).
//!
//! Register with `Application::new().with_assets(BrandAssets)` and use
//! `brand::mascot()` or `brand::symbol()` wherever a GPUI image is accepted.
//! The host application must declare `mod brand;`; this module does not change
//! application startup or apply layout/sizing to its images.

use std::borrow::Cow;

/// Virtual asset paths, independent of the process working directory.
pub const MASCOT: &str = "brand/mascot.svg";
pub const SYMBOL: &str = "brand/symbol.svg";

/// A zero-I/O asset source containing only the two Felifox illustrations.
pub struct BrandAssets;

impl gpui::AssetSource for BrandAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            MASCOT => include_bytes!("../assets/felifox-mascot.svg"),
            SYMBOL => include_bytes!("../assets/felifox-symbol.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        // Match directory boundaries rather than arbitrary string prefixes.
        Ok(match path {
            "" | "/" | "brand" | "brand/" => vec![MASCOT.into(), SYMBOL.into()],
            _ => Vec::new(),
        })
    }
}

/// The full transparent 560 × 420 mascot, with no imposed layout styles.
pub fn mascot() -> gpui::Img {
    gpui::img(gpui::ImageSource::Resource(gpui::Resource::Embedded(
        MASCOT.into(),
    )))
}

/// The transparent 64 × 64 flaming-yarn mark, designed for small UI sizes.
pub fn symbol() -> gpui::Img {
    gpui::img(gpui::ImageSource::Resource(gpui::Resource::Embedded(
        SYMBOL.into(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AssetSource;

    #[test]
    fn embedded_assets_are_borrowed_svg_bytes() {
        for path in [MASCOT, SYMBOL] {
            let bytes = BrandAssets.load(path).unwrap().unwrap();
            assert!(matches!(bytes, Cow::Borrowed(_)));
            let svg = std::str::from_utf8(&bytes).unwrap();
            assert!(svg.contains("<svg "));
            assert!(svg.contains("</svg>"));
        }
    }

    #[test]
    fn lists_only_known_directories() {
        for directory in ["", "/", "brand", "brand/"] {
            let paths = BrandAssets.list(directory).unwrap();
            assert_eq!(paths, vec![gpui::SharedString::from(MASCOT), SYMBOL.into()]);
            for path in paths {
                assert!(BrandAssets.load(path.as_ref()).unwrap().is_some());
            }
        }
        for path in ["bran", "brands", "brand/missing", MASCOT] {
            assert!(BrandAssets.list(path).unwrap().is_empty());
        }
    }

    #[test]
    fn unknown_assets_are_not_found() {
        for path in [
            "",
            "felifox-mascot.svg",
            "brand/missing.svg",
            "../brand/symbol.svg",
        ] {
            assert!(BrandAssets.load(path).unwrap().is_none());
        }
    }

    #[test]
    fn image_helpers_construct_gpui_images() {
        let _: gpui::Img = mascot();
        let _: gpui::Img = symbol();
    }
}
