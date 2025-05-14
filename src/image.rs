use std::path::Path;

use image::{DynamicImage, Rgb};
use image_compare::{Algorithm, Metric, Similarity};
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;

use crate::report::DiffDetail;
use crate::{get_file_name, report};

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
pub enum RGBACompareMode {
    /// full RGBA comparison - probably not intuitive, rarely what you want outside of video processing
    /// Will do MSSIM on luma, then RMS on U and V and alpha channels.
    /// The calculation of the score is then pixel-wise the minimum of each pixels similarity.
    /// To account for perceived indifference in lower alpha regions, this down-weights the difference linearly with mean alpha channel.
    Hybrid,
    /// pre-blend the background in RGBA with this color, use the background RGB values you would assume the pictures to be seen on - usually either black or white
    HybridBlended { r: u8, b: u8, g: u8 },
}

impl Default for RGBACompareMode {
    fn default() -> Self {
        Self::HybridBlended { r: 0, b: 0, g: 0 }
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone, Default)]
pub enum RGBCompareMode {
    ///Comparing rgb images using structure. RGB structure similarity is performed by doing a channel split and taking the maximum deviation (minimum similarity) for the result. The image contains the complete deviations. Algorithm: RMS
    RMS,
    ///Comparing rgb images using structure. RGB structure similarity is performed by doing a channel split and taking the maximum deviation (minimum similarity) for the result. The image contains the complete deviations. Algorithm: MSSIM
    MSSIM,
    ///Comparing structure via MSSIM on Y channel, comparing color-diff-vectors on U and V summing the squares Please mind that the RGBSimilarity-Image does not contain plain RGB here. Probably what you want.
    #[default]
    Hybrid,
}

#[allow(clippy::upper_case_acronyms)]
#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
/// The distance algorithm to use for grayscale comparison, see
/// https://github.com/ChrisRega/image-compare for equations
pub enum GrayStructureAlgorithm {
    /// SSIM with 8x8 pixel windows and averaging over the result
    MSSIM,
    /// Classic RMS distance
    RMS,
}

/// See https://github.com/ChrisRega/image-compare for equations
/// Distance metrics for histograms for grayscale comparison
#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
pub enum GrayHistogramCompareMetric {
    /// Correlation $d(H_1,H_2) = \frac{\sum_I (H_1(I) - \bar{H_1}) (H_2(I) - \bar{H_2})}{\sqrt{\sum_I(H_1(I) - \bar{H_1})^2 \sum_I(H_2(I) - \bar{H_2})^2}}$
    Correlation,
    /// Chi-Square $d(H_1,H_2) = \sum _I \frac{\left(H_1(I)-H_2(I)\right)^2}{H_1(I)}$
    ChiSquare,
    /// Intersection $d(H_1,H_2) = \sum _I \min (H_1(I), H_2(I))$
    Intersection,
    /// Hellinger distance $d(H_1,H_2) = \sqrt{1 - \frac{1}{\sqrt{\int{H_1} \int{H_2}}} \sum_I \sqrt{H_1(I) \cdot H_2(I)}}$
    Hellinger,
}

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
pub enum GrayCompareMode {
    /// Compare gray values pixel structure
    Structure(GrayStructureAlgorithm),
    /// Compare gray values by histogram
    Histogram(GrayHistogramCompareMetric),
}

#[allow(clippy::upper_case_acronyms)]
#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
pub enum CompareMode {
    /// Compare images as RGB
    RGB(RGBCompareMode),
    /// Compare images as RGBA
    RGBA(RGBACompareMode),
    /// Compare images as luminance / grayscale
    Gray(GrayCompareMode),
}

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
/// Image comparison config options
pub struct ImageCompareConfig {
    /// Threshold for image comparison < 0.5 is very dissimilar, 1.0 is identical
    pub threshold: f64,
    #[serde(flatten)]
    /// How to compare the two images
    pub mode: CompareMode,
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

struct ComparisonResult {
    score: f64,
    image: Option<DynamicImage>,
}

impl From<Similarity> for ComparisonResult {
    fn from(value: Similarity) -> Self {
        Self {
            image: Some(value.image.to_color_map()),
            score: value.score,
        }
    }
}

pub fn compare_paths<P: AsRef<Path>>(
    nominal_path: P,
    actual_path: P,
    config: &ImageCompareConfig,
) -> Result<report::Difference, Error> {
    let nominal = image::open(nominal_path.as_ref())?;
    let actual = image::open(actual_path.as_ref())?;
    let result: ComparisonResult = match &config.mode {
        CompareMode::RGBA(c) => {
            let nominal = nominal.into_rgba8();
            let actual = actual.into_rgba8();
            match c {
                RGBACompareMode::Hybrid => {
                    image_compare::rgba_hybrid_compare(&nominal, &actual)?.into()
                }
                RGBACompareMode::HybridBlended { r, g, b } => {
                    image_compare::rgba_blended_hybrid_compare(
                        (&nominal).into(),
                        (&actual).into(),
                        Rgb([*r, *g, *b]),
                    )?
                    .into()
                }
            }
        }
        CompareMode::RGB(c) => {
            let nominal = nominal.into_rgb8();
            let actual = actual.into_rgb8();
            match c {
                RGBCompareMode::RMS => image_compare::rgb_similarity_structure(
                    &Algorithm::RootMeanSquared,
                    &nominal,
                    &actual,
                )?
                .into(),
                RGBCompareMode::MSSIM => image_compare::rgb_similarity_structure(
                    &Algorithm::MSSIMSimple,
                    &nominal,
                    &actual,
                )?
                .into(),
                RGBCompareMode::Hybrid => {
                    image_compare::rgb_hybrid_compare(&nominal, &actual)?.into()
                }
            }
        }
        CompareMode::Gray(c) => {
            let nominal = nominal.into_luma8();
            let actual = actual.into_luma8();
            match c {
                GrayCompareMode::Structure(c) => match c {
                    GrayStructureAlgorithm::MSSIM => image_compare::gray_similarity_structure(
                        &Algorithm::MSSIMSimple,
                        &nominal,
                        &actual,
                    )?
                    .into(),
                    GrayStructureAlgorithm::RMS => image_compare::gray_similarity_structure(
                        &Algorithm::RootMeanSquared,
                        &nominal,
                        &actual,
                    )?
                    .into(),
                },
                GrayCompareMode::Histogram(c) => {
                    let metric = match c {
                        GrayHistogramCompareMetric::Correlation => Metric::Correlation,
                        GrayHistogramCompareMetric::ChiSquare => Metric::ChiSquare,
                        GrayHistogramCompareMetric::Intersection => Metric::Intersection,
                        GrayHistogramCompareMetric::Hellinger => Metric::Hellinger,
                    };
                    let score =
                        image_compare::gray_similarity_histogram(metric, &nominal, &actual)?;
                    ComparisonResult { score, image: None }
                }
            }
        }
    };

    let mut result_diff = report::Difference::new_for_file(&nominal_path, &actual_path);
    if result.score < config.threshold {
        let out_path_set = if let Some(i) = result.image {
            let nominal_file_name =
                get_file_name(nominal_path.as_ref()).ok_or(Error::FileNameParsing(format!(
                    "Could not extract filename from path {:?}",
                    nominal_path.as_ref()
                )))?;
            let out_path = (nominal_file_name + "diff_image.png").to_string();
            i.save(&out_path)?;
            Some(out_path)
        } else {
            None
        };

        let error_message = format!(
            "Diff for image {} was not met, expected {}, found {}",
            nominal_path.as_ref().to_string_lossy(),
            config.threshold,
            result.score
        );
        error!("{}", &error_message);

        result_diff.push_detail(DiffDetail::Image {
            diff_image: out_path_set,
            score: result.score,
        });
        result_diff.error();
    }
    Ok(result_diff)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn identity() {
        let result = compare_paths(
            "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg",
            "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg",
            &ImageCompareConfig {
                threshold: 1.0,
                mode: CompareMode::RGB(RGBCompareMode::Hybrid),
            },
        )
        .unwrap();
        assert!(!result.is_error);
    }

    #[test]
    fn pin_diff_image() {
        let result = compare_paths(
            "tests/integ/data/images/expected/SaveImage_100DPI_default_size.jpg",
            "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg",
            &ImageCompareConfig {
                threshold: 1.0,
                mode: CompareMode::RGBA(RGBACompareMode::Hybrid),
            },
        )
        .unwrap();
        assert!(result.is_error);
        if let DiffDetail::Image {
            score: _,
            diff_image,
        } = result.detail.first().unwrap()
        {
            let img = image::open(diff_image.as_ref().unwrap())
                .unwrap()
                .into_rgba8();
            let nom = image::open("tests/integ/data/images/diff_100_DPI.png")
                .unwrap()
                .into_rgba8();
            let diff_result = image_compare::rgba_hybrid_compare(&img, &nom)
                .expect("Wrong dimensions of diff images!");
            assert_eq!(diff_result.score, 1.0);
        } else {
            unreachable!();
        }
    }
}
