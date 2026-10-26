use hostvitals_policy::redaction::redact;
use proptest::prelude::*;

proptest! {
    #[test]
    fn secret_values_are_withheld_and_nonsecret_arguments_are_exact(
        value in "PRIVATE_[a-zA-Z0-9 \\t=:/@💡-]{1,70}_END",
        public in "PUBLIC_[a-zA-Z0-9 \\t=💡-]{0,70}",
        flag in prop::sample::select(vec!["--token", "--API_KEY", "--client-secret", "--Password", "--passwd", "--AUTH", "--credential"]),
    ) {
        let args = vec!["program".into(), format!("{flag}={value}"), flag.into(), value.clone(), "--label".into(), public.clone()];
        let answer = redact(&args);
        prop_assert_eq!(&answer, &vec!["program".into(), format!("{flag}=<redacted>"), flag.into(), "<redacted>".into(), "--label".into(), public]);
        prop_assert!(!answer.join(" ").contains(&value));
    }

    #[test]
    fn url_user_and_password_never_survive(user in "PRIVATE_USER_[a-z]{1,30}", pass in "PRIVATE_PASS_[a-z]{1,30}") {
        let answer = redact(&[format!("--endpoint=https://{user}:{pass}@example.test:8443/a?q=ok"), "ftp://alice@example.test/file".into()]);
        prop_assert_eq!(&answer, &vec!["--endpoint=https://<redacted>@example.test:8443/a?q=ok".to_string(), "ftp://<redacted>@example.test/file".to_string()]);
        prop_assert!(!answer.join(" ").contains(&user));
        prop_assert!(!answer.join(" ").contains(&pass));
    }
}

#[test]
fn empty_and_flag_like_secrets_keep_their_argument_boundaries() {
    let input: Vec<String> = [
        "p",
        "--password",
        "",
        "--token",
        "--looks-like-a-flag",
        "--key=",
        "",
        "two words",
        "--label=ok",
        "https://u:p@host/a",
        "https://host/a@b",
    ]
    .map(str::to_owned)
    .into();
    assert_eq!(
        redact(&input),
        [
            "p",
            "--password",
            "<redacted>",
            "--token",
            "<redacted>",
            "--key=<redacted>",
            "",
            "two words",
            "--label=ok",
            "https://<redacted>@host/a",
            "https://host/a@b"
        ]
    );
}

#[test]
fn multiple_urls_in_one_argument_are_each_sanitized() {
    assert_eq!(
        redact(&["https://private:secret@one.test https://other:password@two.test/a".into()]),
        ["https://<redacted>@one.test https://<redacted>@two.test/a"]
    );
}
