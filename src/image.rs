use crate::report::DiffDetail;
use crate::{get_file_name, report};
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::error;

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
/// Image comparison config options
pub struct ImageCompareConfig {
    /// Threshold for image comparison < 0.5 is very dissimilar, 1.0 is identical
    pub threshold: f64,
}

impl ImageCompareConfig {
    /// create an [`ImageCompareConfig`] given the threshold
    pub fn from_threshold(threshold: f64) -> Self {
        ImageCompareConfig { threshold }
    }
}

impl Default for ImageCompareConfig {
    fn default() -> Self {
        ImageCompareConfig::from_threshold(1.0)
    }
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

pub fn compare_paths<P: AsRef<Path>>(
    nominal_path: P,
    actual_path: P,
    config: &ImageCompareConfig,
) -> Result<report::Difference, Error> {
    let nominal = image::open(nominal_path.as_ref())?.into_rgba8();
    let actual = image::open(actual_path.as_ref())?.into_rgba8();

    let result = image_compare::rgba_hybrid_compare(&nominal, &actual)?;
    let nominal_file_name =
        get_file_name(nominal_path.as_ref()).ok_or(Error::FileNameParsing(format!(
            "Could not extract filename from path {:?}",
            nominal_path.as_ref()
        )))?;
    let out_path = (nominal_file_name + "diff_image.png").to_string();
    let mut result_diff = report::Difference::new_for_file(&nominal_path, &actual_path);

    if result.score < config.threshold {
        let color_map = result.image.to_color_map();
        color_map.save(PathBuf::from(&out_path))?;

        let error_message = format!(
            "Diff for image {} was not met, expected {}, found {}",
            nominal_path.as_ref().to_string_lossy(),
            config.threshold,
            result.score
        );
        error!("{}", &error_message);
        result_diff.push_detail(DiffDetail::Image {
            diff_image: out_path,
            score: result.score,
        });
        result_diff.error();
    }
    Ok(result_diff)
}

#[cfg(test)]
mod test {
    use crate::image::{compare_paths, ImageCompareConfig};
    use crate::report::DiffDetail;

    #[test]
    fn identity() {
        let result = compare_paths(
            "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg",
            "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg",
            &ImageCompareConfig { threshold: 1.0 },
        )
        .unwrap();
        assert!(!result.is_error);
    }

    #[ignore] // TODO fix on arm mac
    #[test]
    fn pin_diff_image() {
        let result = compare_paths(
            "tests/integ/data/images/expected/SaveImage_100DPI_default_size.jpg",
            "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg",
            &ImageCompareConfig { threshold: 1.0 },
        )
        .unwrap();
        assert!(result.is_error);
        if let DiffDetail::Image {
            score: _,
            diff_image,
        } = result.detail.first().unwrap()
        {
            let img = image::open(diff_image).unwrap().into_rgb8();
            let nom = image::open("tests/integ/data/images/diff_100_DPI.png")
                .unwrap()
                .into_rgb8();
            let diff_result = image_compare::rgb_hybrid_compare(&img, &nom)
                .expect("Wrong dimensions of diff images!");
            assert_eq!(diff_result.score, 1.0);
        } else {
            unreachable!();
        }
    }
}
