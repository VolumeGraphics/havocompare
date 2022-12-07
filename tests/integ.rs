use havocompare::compare_folders;
use test_log::test;

#[test]
fn simple_test_identity() {
    let report_dir = tempdir::TempDir::new("hvc_testing")
        .expect("Could not generate temporary directory for report");
    let result = compare_folders("tests/", "tests/", "tests/integ/config.yml", report_dir);
    assert!(result.unwrap());
}

#[test]
fn display_of_status_message_in_cm_tables() {
    let report_dir = tempdir::TempDir::new("hvc_testing")
        .expect("Could not generate temporary directory for report");

    assert!(compare_folders(
        "tests/integ/data/display_of_status_message_in_cm_tables/expected/",
        "tests/integ/data/display_of_status_message_in_cm_tables/actual/",
        "tests/integ/vgrf.yml",
        report_dir
    )
    .unwrap());
}

#[test]
fn images_test() {
    let report_dir = tempdir::TempDir::new("hvc_testing")
        .expect("Could not generate temporary directory for report");

    assert!(compare_folders(
        "tests/integ/data/images/expected/",
        "tests/integ/data/images/actual/",
        "tests/integ/jpg_compare.yml",
        report_dir
    )
    .unwrap());
}
