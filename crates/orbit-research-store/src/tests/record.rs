use std::process::Command;

#[test]
fn utc_date_matches_the_system_clock_in_iso_form() {
    let expected = Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .expect("date should be installed");
    assert!(expected.status.success());
    let expected = String::from_utf8_lossy(&expected.stdout).trim().to_owned();

    assert_eq!(crate::record::utc_date().unwrap(), expected);
}
