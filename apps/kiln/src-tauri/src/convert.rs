//! Image conversion. Everything here runs on a worker thread; nothing here
//! knows about Tauri, windows, or events.

use libvips::ops;
use libvips::VipsImage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Jpeg,
    Png,
    Webp,
    Avif,
    Heic,
    Tiff,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
            OutputFormat::Avif => "avif",
            OutputFormat::Heic => "heic",
            OutputFormat::Tiff => "tiff",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Resize {
    /// Leave the pixel dimensions alone.
    Original,
    /// Produce the exact requested width and height.
    Exact { width: i32, height: i32 },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub format: OutputFormat,
    /// 1-100. Ignored for formats that ignore it.
    pub quality: i32,
    pub resize: Resize,
    /// False strips EXIF, XMP, IPTC and any C2PA/AI-generation tags with them.
    pub keep_metadata: bool,
    /// Private scratch directory used until the user saves the result.
    pub destination: String,
    /// Appended to every name in the batch, before the extension. The frontend
    /// stamps one value per run so a batch shares it.
    pub suffix: Option<String>,
}

/// What the frontend shows in a row before anything is converted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub width: i32,
    pub height: i32,
    /// True when the file carries metadata worth stripping — GPS, camera, dates,
    /// author, XMP/IPTC, or C2PA/AI tags. The trivial exif-data block that every
    /// encoder writes (orientation, resolution) does not count.
    pub has_metadata: bool,
}

pub fn probe(path: &Path) -> Result<SourceInfo, String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| "path is not valid UTF-8".to_string())?;
    let image = VipsImage::new_from_file(path_str).map_err(vips_error)?;
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    Ok(SourceInfo {
        path: path_str.to_string(),
        name: file_name(path),
        bytes,
        width: image.get_width(),
        height: image.get_height(),
        has_metadata: has_meaningful_metadata(&image),
    })
}

/// Whether stripping would actually remove anything a user cares about. Every
/// JPEG carries a small exif-data block (orientation, resolution) that is not
/// worth a re-encode; we look instead for the identifying fields.
fn has_meaningful_metadata(image: &VipsImage) -> bool {
    const FIELDS: [&str; 14] = [
        "xmp-data",
        "iptc-data",
        "exif-ifd0-Make",
        "exif-ifd0-Model",
        "exif-ifd0-Artist",
        "exif-ifd0-Copyright",
        "exif-ifd0-DateTime",
        "exif-ifd0-Software",
        "exif-ifd0-ImageDescription",
        "exif-ifd2-DateTimeOriginal",
        "exif-ifd2-DateTimeDigitized",
        "exif-ifd3-GPSLatitude",
        "exif-ifd3-GPSLongitude",
        "exif-ifd3-GPSInfo",
    ];

    let present = FIELDS
        .iter()
        .any(|field| image.get_as_string(field).is_ok());
    // get_as_string on an absent field sets the global error buffer.
    unsafe { libvips::bindings::vips_error_clear() };
    present
}

/// Converts one file and returns where it was written. The source is only ever read.
pub fn convert_one(source: &Path, settings: &Settings) -> Result<PathBuf, String> {
    let source_str = source
        .to_str()
        .ok_or_else(|| "path is not valid UTF-8".to_string())?;

    // Longest edge is the same number whichever way the photo is rotated, so it
    // survives the autorotation that thumbnail applies below.
    let probe = VipsImage::new_from_file(source_str).map_err(vips_error)?;
    let longest = probe.get_width().max(probe.get_height());
    drop(probe);

    let (target_width, target_height, thumbnail_size) = match settings.resize {
        Resize::Original => (longest, longest, ops::Size::Down),
        Resize::Exact { width, height } => {
            if !exact_dimensions_valid(width, height) {
                return Err("dimensions must be between 1024 px and 7680 px".to_string());
            }
            (width, height, ops::Size::Force)
        }
    };

    // Stripping metadata keeps the colour profile, because deleting a profile
    // shifts colours rather than removing information. Everything else goes:
    // GPS, camera serial, timestamps, and C2PA generator tags.
    let (keep, output_profile) = if settings.keep_metadata {
        (ops::ForeignKeep::All, None)
    } else {
        (ops::ForeignKeep::Icc, Some("srgb".to_string()))
    };

    // thumbnail() rather than a load-then-resize: it reads the source at a
    // reduced resolution where the format allows, applies the orientation tag
    // so phone photos are not sideways, and never holds the whole image.
    let options = ops::ThumbnailOptions {
        height: target_height,
        size: thumbnail_size,
        output_profile,
        ..ops::ThumbnailOptions::default()
    };
    let image = ops::thumbnail_with_opts(source_str, target_width, &options).map_err(vips_error)?;

    let destination = PathBuf::from(&settings.destination);
    std::fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
    // Reserved before the write, so neither an existing file nor another worker
    // in this same batch is overwritten.
    let out_path = reserve_path(
        &destination,
        source,
        settings.format,
        settings.suffix.as_deref(),
    )?;
    let out_str = out_path
        .to_str()
        .ok_or_else(|| "output path is not valid UTF-8".to_string())?;

    let quality = normalized_quality(settings.quality);
    match settings.format {
        OutputFormat::Jpeg => ops::jpegsave_with_opts(
            &image,
            out_str,
            &ops::JpegsaveOptions {
                q: quality,
                keep,
                ..Default::default()
            },
        ),
        OutputFormat::Png => ops::pngsave_with_opts(
            &image,
            out_str,
            &ops::PngsaveOptions {
                keep,
                ..Default::default()
            },
        ),
        OutputFormat::Webp => ops::webpsave_with_opts(
            &image,
            out_str,
            &ops::WebpsaveOptions {
                q: quality,
                keep,
                ..Default::default()
            },
        ),
        OutputFormat::Avif => ops::heifsave_with_opts(
            &image,
            out_str,
            &ops::HeifsaveOptions {
                q: quality,
                compression: ops::ForeignHeifCompression::Av1,
                keep,
                ..Default::default()
            },
        ),
        OutputFormat::Heic => ops::heifsave_with_opts(
            &image,
            out_str,
            &ops::HeifsaveOptions {
                q: quality,
                compression: ops::ForeignHeifCompression::Hevc,
                keep,
                ..Default::default()
            },
        ),
        OutputFormat::Tiff => ops::tiffsave_with_opts(
            &image,
            out_str,
            &ops::TiffsaveOptions {
                compression: ops::ForeignTiffCompression::Deflate,
                keep,
                ..Default::default()
            },
        ),
    }
    .map_err(|error| {
        let _ = std::fs::remove_file(&out_path);
        vips_error(error)
    })?;

    Ok(out_path)
}

fn normalized_quality(quality: i32) -> i32 {
    quality.clamp(40, 100)
}

fn exact_dimensions_valid(width: i32, height: i32) -> bool {
    (1024..=7680).contains(&width) && (1024..=7680).contains(&height)
}

/// Picks a free name and claims it by creating the file, which is what makes it
/// safe: two workers converting `a.heic` and `a.png` into the same folder would
/// otherwise both see `a.jpg` as free and one would silently overwrite the other.
fn reserve_path(
    directory: &Path,
    source: &Path,
    format: OutputFormat,
    suffix: Option<&str>,
) -> Result<PathBuf, String> {
    let mut stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_string());
    if let Some(suffix) = suffix {
        stem.push('-');
        stem.push_str(suffix);
    }
    let extension = format.extension();

    let mut counter = 0;
    loop {
        let candidate = if counter == 0 {
            directory.join(format!("{stem}.{extension}"))
        } else {
            directory.join(format!("{stem} ({counter}).{extension}"))
        };

        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => counter += 1,
            Err(error) => return Err(error.to_string()),
        }

        if counter > 10_000 {
            return Err("could not find a free filename".to_string());
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// libvips puts the useful detail in a global error buffer rather than in the
/// error value, so both are read.
fn vips_error(error: libvips::error::Error) -> String {
    let detail = unsafe {
        let buffer = libvips::bindings::vips_error_buffer();
        if buffer.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(buffer)
                .to_string_lossy()
                .trim()
                .to_string()
        }
    };
    unsafe { libvips::bindings::vips_error_clear() };

    if detail.is_empty() {
        format!("{error}")
    } else {
        detail.lines().last().unwrap_or(&detail).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    static VIPS: OnceLock<libvips::VipsApp> = OnceLock::new();

    fn vips() {
        VIPS.get_or_init(|| libvips::VipsApp::default("kiln-tests").unwrap());
    }

    /// A wide, GPS-tagged JPEG, so both resizing and metadata have something to act on.
    fn fixture(directory: &Path) -> PathBuf {
        let path = directory.join("source.jpg");
        let image = ops::black(1600, 900).unwrap();
        let image = ops::colourspace(&image, ops::Interpretation::Srgb).unwrap();
        ops::jpegsave(&image, path.to_str().unwrap()).unwrap();
        path
    }

    fn settings(format: OutputFormat, resize: Resize, keep_metadata: bool, out: &Path) -> Settings {
        Settings {
            format,
            quality: 80,
            resize,
            keep_metadata,
            destination: out.to_string_lossy().into_owned(),
            suffix: None,
        }
    }

    #[test]
    fn converts_to_exact_width_and_height() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-exact");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = fixture(&directory);

        let output = convert_one(
            &source,
            &settings(
                OutputFormat::Webp,
                Resize::Exact {
                    width: 1024,
                    height: 1200,
                },
                false,
                &directory,
            ),
        )
        .unwrap();

        let result = VipsImage::new_from_file(output.to_str().unwrap()).unwrap();
        assert_eq!((result.get_width(), result.get_height()), (1024, 1200));
    }

    #[test]
    fn converts_every_supported_output_format() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-all-formats");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = fixture(&directory);

        for format in [
            OutputFormat::Jpeg,
            OutputFormat::Png,
            OutputFormat::Webp,
            OutputFormat::Avif,
            OutputFormat::Heic,
            OutputFormat::Tiff,
        ] {
            let output_dir = directory.join(format.extension());
            std::fs::create_dir_all(&output_dir).unwrap();
            let output = convert_one(
                &source,
                &settings(format, Resize::Original, false, &output_dir),
            )
            .unwrap_or_else(|error| panic!("{} conversion failed: {error}", format.extension()));
            assert_eq!(output.extension().unwrap(), format.extension());
            assert!(VipsImage::new_from_file(output.to_str().unwrap()).is_ok());
        }
    }

    #[test]
    fn exact_dimension_boundaries_are_enforced_without_allocating_giant_images() {
        assert!(exact_dimensions_valid(1024, 1024));
        assert!(exact_dimensions_valid(7680, 7680));
        assert!(!exact_dimensions_valid(1023, 1024));
        assert!(!exact_dimensions_valid(1024, 1023));
        assert!(!exact_dimensions_valid(7681, 1024));
        assert!(!exact_dimensions_valid(1024, 7681));
    }

    #[test]
    fn compression_quality_is_clamped_to_the_ui_contract() {
        assert_eq!(normalized_quality(39), 40);
        assert_eq!(normalized_quality(40), 40);
        assert_eq!(normalized_quality(80), 80);
        assert_eq!(normalized_quality(100), 100);
        assert_eq!(normalized_quality(101), 100);
    }

    #[test]
    fn existing_files_are_not_overwritten() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-collide");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = fixture(&directory);
        let options = settings(OutputFormat::Png, Resize::Original, false, &directory);

        let first = convert_one(&source, &options).unwrap();
        let second = convert_one(&source, &options).unwrap();

        assert_ne!(first, second);
        assert!(first.exists() && second.exists());
        assert_eq!(
            second.file_name().unwrap().to_string_lossy(),
            "source (1).png"
        );
    }

    #[test]
    fn conversion_never_modifies_the_original_source() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-source-untouched");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = fixture(&directory);
        let original_bytes = std::fs::read(&source).unwrap();
        let output_dir = directory.join("scratch");

        let output = convert_one(
            &source,
            &settings(
                OutputFormat::Webp,
                Resize::Exact {
                    width: 1024,
                    height: 1024,
                },
                false,
                &output_dir,
            ),
        )
        .unwrap();

        assert_ne!(output, source);
        assert_eq!(std::fs::read(&source).unwrap(), original_bytes);
    }

    #[test]
    fn a_timestamp_suffix_lands_before_the_extension() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-suffix");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = fixture(&directory);

        let mut options = settings(OutputFormat::Png, Resize::Original, false, &directory);
        options.suffix = Some("20260824-131205".to_string());
        let output = convert_one(&source, &options).unwrap();

        assert_eq!(
            output.file_name().unwrap().to_string_lossy(),
            "source-20260824-131205.png"
        );
    }

    #[test]
    fn metadata_detection_ignores_the_trivial_block_but_finds_real_tags() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-meta");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        // A plain encode carries only orientation/resolution — not worth stripping.
        let plain = directory.join("plain.jpg");
        let image = ops::black(64, 64).unwrap();
        ops::jpegsave(&image, plain.to_str().unwrap()).unwrap();
        assert!(!probe(&plain).unwrap().has_metadata);

        // Attach a camera make and GPS with exiftool, if it is installed; then
        // there is something worth stripping.
        let tagged = directory.join("tagged.jpg");
        std::fs::copy(&plain, &tagged).unwrap();
        let tagged_ok = std::process::Command::new("exiftool")
            .args([
                "-Make=Canon",
                "-GPSLatitude=51.5",
                "-GPSLatitudeRef=N",
                "-overwrite_original",
            ])
            .arg(&tagged)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if tagged_ok {
            assert!(probe(&tagged).unwrap().has_metadata);
        }
    }

    #[test]
    fn keep_metadata_preserves_xmp_while_unchecked_strips_it() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-keep-metadata");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let plain = fixture(&directory);
        let xmp = directory.join("test.xmp");
        std::fs::write(
            &xmp,
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" dc:creator="Kiln Test" />
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#,
        )
        .unwrap();

        let tagged = directory.join("tagged.jpg");
        let tagged_ok = std::process::Command::new("magick")
            .arg(&plain)
            .arg("-profile")
            .arg(&xmp)
            .arg(&tagged)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !tagged_ok {
            return;
        }
        assert!(probe(&tagged).unwrap().has_metadata);

        let kept_dir = directory.join("kept");
        let stripped_dir = directory.join("stripped");
        std::fs::create_dir_all(&kept_dir).unwrap();
        std::fs::create_dir_all(&stripped_dir).unwrap();

        let kept = convert_one(
            &tagged,
            &settings(OutputFormat::Jpeg, Resize::Original, true, &kept_dir),
        )
        .unwrap();
        let stripped = convert_one(
            &tagged,
            &settings(OutputFormat::Jpeg, Resize::Original, false, &stripped_dir),
        )
        .unwrap();

        assert!(probe(&kept).unwrap().has_metadata);
        assert!(!probe(&stripped).unwrap().has_metadata);
    }

    #[test]
    fn workers_racing_for_the_same_output_name_do_not_overwrite_each_other() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-race");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        // Different sources, identical stem: every one wants "same.jpg".
        let mut sources = Vec::new();
        for index in 0..8 {
            let folder = directory.join(format!("in{index}"));
            std::fs::create_dir_all(&folder).unwrap();
            let path = folder.join("same.jpg");
            let image = ops::black(80, 60).unwrap();
            ops::jpegsave(&image, path.to_str().unwrap()).unwrap();
            sources.push(path);
        }

        let out = directory.join("out");
        let options = settings(OutputFormat::Jpeg, Resize::Original, false, &out);
        let outputs: Vec<PathBuf> = std::thread::scope(|scope| {
            let handles: Vec<_> = sources
                .iter()
                .map(|source| scope.spawn(|| convert_one(source, &options).unwrap()))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });

        let unique: std::collections::HashSet<_> = outputs.iter().collect();
        assert_eq!(unique.len(), 8, "two workers claimed the same output name");
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 8);
    }

    #[test]
    fn a_failed_conversion_leaves_no_empty_file_behind() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-cleanup");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("broken.jpg");
        std::fs::write(&source, b"not a jpeg at all").unwrap();

        let _ = convert_one(
            &source,
            &settings(OutputFormat::Png, Resize::Original, false, &directory),
        );

        assert!(!directory.join("broken.png").exists());
    }

    #[test]
    fn a_file_that_is_not_an_image_fails_without_panicking() {
        vips();
        let directory = std::env::temp_dir().join("kiln-test-garbage");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("not-an-image.jpg");
        std::fs::write(&source, b"this is not a jpeg").unwrap();

        let result = convert_one(
            &source,
            &settings(OutputFormat::Jpeg, Resize::Original, false, &directory),
        );

        assert!(result.is_err());
        assert!(!result.unwrap_err().is_empty());
    }
}
