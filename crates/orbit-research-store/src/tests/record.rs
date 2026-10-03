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

#[test]
fn slugs_transliterate_and_fall_back_to_a_valid_name() {
    use crate::record::slug_for;
    let valid = |slug: &str| {
        !slug.is_empty()
            && !slug.starts_with('-')
            && !slug.ends_with('-')
            && !slug.contains("--")
            && slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    assert_eq!(slug_for("Ünïcödé"), "unicode");
    assert_eq!(slug_for("Café Zürich — ça va?"), "cafe-zurich-ca-va");
    assert_eq!(slug_for("Plain ASCII title 2"), "plain-ascii-title-2");
    assert_eq!(slug_for("Why does CI flake"), "why-does-ci-flake");
    // CJK text is romanized rather than refused, and nothing pronounceable
    // falls back to a fixed name.
    assert_eq!(slug_for("日本語"), "ri-ben-yu");
    assert_eq!(slug_for("🚀"), "rocket");
    assert_eq!(slug_for("\u{200b}"), "untitled");
    for title in ["Ünïcödé", "日本語の質問", "🚀", "***", "\u{200b}", "a  b"] {
        assert!(valid(&slug_for(title)), "{title}: {}", slug_for(title));
    }
}

#[test]
fn non_ascii_titles_are_capturable_and_declare_their_slug() {
    use crate::{corpus::Corpus, tests::writer::fixture};
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    for (key, title, path) in [
        ("u1", "Ünïcödé", "questions/Q001-unicode.md"),
        ("u2", "日本語", "questions/Q002-ri-ben-yu.md"),
        ("u3", "\u{200b}\u{200b}", "questions/Q003-untitled.md"),
        ("u4", "Plain", "questions/Q004-plain.md"),
    ] {
        let reservation = corpus
            .reserve(key, "Q", title, "b", vec![], vec![])
            .unwrap();
        assert_eq!(reservation.path, path);
    }
    let snapshot = corpus.snapshot().expect("the corpus validates");
    let slug = |id: &str| {
        snapshot
            .records
            .iter()
            .find(|r| r.id == id)
            .unwrap()
            .metadata["slug"]
            .clone()
    };
    // A transliterated slug is declared, so a checker that compares the path
    // to the plain kebab-case of the title still accepts the record.
    assert_eq!(slug("Q001"), "unicode");
    assert_eq!(slug("Q002"), "ri-ben-yu");
    assert_eq!(slug("Q003"), "untitled");
    assert!(slug("Q004").is_null());
}
