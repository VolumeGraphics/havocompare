use crate::report;
use image_compare::ToColorMap;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{error, info};

#[derive(JsonSchema, Deserialize, Serialize, Debug)]
/// Image comparison config options
pub struct ImageCompareConfig {
    /// Threshold for image comparison < 0.5 is very dissimilar, 1.0 is identical
    pub threshold: f64,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Error loading image {0}")]
    ImageDecoding(#[from] image::ImageError),
    #[error("Problem creating hash report {0}")]
    Reporting(#[from] report::Error),
    #[error("Image comparison algorithm failed {0}")]
    ImageComparison(#[from] image_compare::CompareError),
    #[error("Problem processing file name {0}")]
    FileNameParsing(String),
}

fn get_file_name(path: &Path) -> Result<Cow<str>, Error> {
    path.file_name()
        .map(|f| f.to_string_lossy())
        .ok_or_else(|| {
            Error::FileNameParsing(format!(
                "Could not extract filename from {}",
                path.to_string_lossy()
            ))
        })
}

pub fn compare_paths<P: AsRef<Path>>(
    nominal_path: P,
    actual_path: P,
    config: &ImageCompareConfig,
    rule_name: &str,
) -> Result<report::FileCompareResult, Error> {
    let mut diffs: Vec<String> = Vec::new();
    let nominal = image::open(nominal_path.as_ref())?.into_rgb8();
    let actual = image::open(actual_path.as_ref())?.into_rgb8();

    let result = image_compare::rgb_hybrid_compare(&nominal, &actual)?;
    if result.score < config.threshold {
        let color_map = result.image.to_color_map();
        let nominal_file_name = get_file_name(nominal_path.as_ref())?;
        let out_path = (nominal_file_name + "diff_image.png").to_string();
        info!("Writing diff image to: {}", out_path.as_str());
        color_map.save(PathBuf::from(&out_path))?;

        let error_message = format!(
            "Diff for image {} was not met, expected {}, found {}",
            nominal_path.as_ref().to_string_lossy(),
            config.threshold,
            result.score
        );

        error!("{}", &error_message);

        diffs.push(error_message);
        diffs.push(out_path);
    }

    Ok(report::write_image_detail(
        nominal_path.as_ref(),
        actual_path.as_ref(),
        &diffs,
        rule_name,
    )?)
}

#[cfg(test)]
mod test {
    use crate::image::{compare_paths, ImageCompareConfig};

    #[test]
    fn identity() {
        let result = compare_paths(
            "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg",
            "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg",
            &ImageCompareConfig { threshold: 1.0 },
            "test-rule",
        )
        .unwrap();
        assert!(!result.is_error);
    }

    #[test]
    fn pin_diff_image() {
        let result = compare_paths(
            "tests/integ/data/images/expected/SaveImage_100DPI_default_size.jpg",
            "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg",
            &ImageCompareConfig { threshold: 1.0 },
            "test-rule",
        )
        .unwrap();
        assert!(result.is_error);
        assert!(result.detail_path.is_some());
        let img = image::open(
            result
                .detail_path
                .unwrap()
                .join("SaveImage_100DPI_default_size.jpgdiff_image.png"),
        )
        .expect("Could not load generated diff image")
        .into_rgb8();
        let nom = image::open("tests/integ/data/images/diff_100_DPI.png")
            .unwrap()
            .into_rgb8();
        let diff_result = image_compare::rgb_hybrid_compare(&img, &nom)
            .expect("Wrong dimensions of diff images!");
        assert_eq!(diff_result.score, 1.0);
    }
}
