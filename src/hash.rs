use crate::{report, Deserialize, Serialize};
use data_encoding::HEXLOWER;

use schemars_derive::JsonSchema;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub enum HashFunction {
    Sha256,
}

impl HashFunction {
    fn hash_file(&self, mut file: impl Read) -> [u8; 32] {
        match self {
            Self::Sha256 => {
                use sha2::{Digest, Sha256};
                use std::io;

                let mut hasher = Sha256::new();

                let _ = io::copy(&mut file, &mut hasher).expect("Could not open file to hash");
                let hash_bytes = hasher.finalize();
                hash_bytes.into()
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HashConfig {
    hash: HashFunction,
}

impl Default for HashConfig {
    fn default() -> Self {
        HashConfig {
            hash: HashFunction::Sha256,
        }
    }
}

pub fn compare_files<P: AsRef<Path>>(
    actual_path: P,
    nominal_path: P,
    config: &HashConfig,
    rule_name: &str,
) -> report::FileCompareResult {
    let act = config.hash.hash_file(
        File::open(actual_path.as_ref()).expect("Could not open actual file for hashing"),
    );
    let nom = config.hash.hash_file(
        File::open(nominal_path.as_ref()).expect("Could not open actual file for hashing"),
    );

    let diff = if act != nom {
        vec![format!(
            "Nominal file's hash is '{}' actual is '{}'",
            HEXLOWER.encode(&act),
            HEXLOWER.encode(&nom)
        )]
    } else {
        vec![]
    };

    report::write_html_detail(nominal_path, actual_path, diff.as_slice(), rule_name)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::hash::HashFunction::Sha256;

    #[test]
    fn identity() {
        let f1 = Sha256.hash_file(File::open("tests/integ.rs").unwrap());
        let f2 = Sha256.hash_file(File::open("tests/integ.rs").unwrap());
        assert_eq!(f1, f2);
    }

    #[test]
    fn hash_pinning() {
        let sum = "bc3abb411d305c4436185c474be3db2608e910612a573f6791b143d7d749b699";
        let f1 = Sha256.hash_file(File::open("tests/integ/data/images/diff_100_DPI.png").unwrap());
        assert_eq!(HEXLOWER.encode(&f1), sum);
    }

    #[test]
    fn identity_outer() {
        let file = "tests/integ.rs";
        let result = compare_files(file, file, &HashConfig::default(), "test");
        assert!(!result.is_error);
    }

    #[test]
    fn different_files_throw_outer() {
        let file_act = "tests/integ/data/images/actual/SaveImage_100DPI_default_size.jpg";
        let file_nominal = "tests/integ/data/images/expected/SaveImage_100DPI_default_size.jpg";

        let result = compare_files(file_act, file_nominal, &HashConfig::default(), "test");
        assert!(result.is_error);
    }
}
