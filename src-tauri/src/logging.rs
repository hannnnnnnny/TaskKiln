//! Log sanitization. Everything TaskKiln persists or displays from a child
//! process passes through [`sanitize`] so obvious secrets never hit SQLite.

use std::sync::OnceLock;

use regex::Regex;

const MAX_LINE: usize = 4_000;

fn patterns() -> &'static [(Regex, &'static str)] {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        let rules: &[(&str, &str)] = &[
            (r"sk-ant-[A-Za-z0-9_\-]{8,}", "sk-ant-[REDACTED]"),
            (r"\bsk-[A-Za-z0-9_\-]{20,}", "sk-[REDACTED]"),
            (r"\bgh[pousr]_[A-Za-z0-9]{20,}", "gh_[REDACTED]"),
            (r"\bgithub_pat_[A-Za-z0-9_]{20,}", "github_pat_[REDACTED]"),
            (r"\bAKIA[0-9A-Z]{16}\b", "AKIA[REDACTED]"),
            (r"\bxox[abpr]-[A-Za-z0-9\-]{10,}", "xox-[REDACTED]"),
            (r"(?i)\b(bearer)\s+[A-Za-z0-9._\-+/=]{12,}", "$1 [REDACTED]"),
            (
                r#"(?i)\b([A-Z0-9_]*(?:API_KEY|APIKEY|SECRET|TOKEN|PASSWORD|PASSWD))\s*([=:])\s*["']?[^\s"']{4,}"#,
                "$1$2[REDACTED]",
            ),
            (r"-----BEGIN [A-Z ]*PRIVATE KEY-----", "[REDACTED PRIVATE KEY]"),
        ];
        rules
            .iter()
            .map(|(re, rep)| (Regex::new(re).expect("static regex"), *rep))
            .collect()
    })
}

pub fn sanitize(input: &str) -> String {
    let mut out = input.to_string();
    for (re, rep) in patterns() {
        if re.is_match(&out) {
            out = re.replace_all(&out, *rep).into_owned();
        }
    }
    truncate(&out, MAX_LINE)
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated {} bytes]", &s[..end], s.len() - end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_secrets() {
        let cases = [
            ("key sk-ant-api03-abcdefghijklmnop", "sk-ant-abc"),
            ("ghp_abcdefghijklmnopqrstuvwxyz0123", "ghp_abc"),
            ("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.abc", "eyJhbGci"),
            ("ANTHROPIC_API_KEY=supersecretvalue", "supersecret"),
            ("export DB_PASSWORD: 'hunter2hunter2'", "hunter2"),
            ("AKIAABCDEFGHIJKLMNOP", "AKIAABCDEFGH"),
        ];
        for (input, leaked) in cases {
            let out = sanitize(input);
            assert!(!out.contains(leaked), "{input} -> {out}");
            assert!(out.contains("REDACTED"), "{input} -> {out}");
        }
    }

    #[test]
    fn leaves_normal_text_alone() {
        let line = "editing src/auth/session.ts; 18 passed / 0 failed";
        assert_eq!(sanitize(line), line);
    }

    #[test]
    fn truncates_on_char_boundary() {
        let s = "é".repeat(5000);
        let out = truncate(&s, 101);
        assert!(out.contains("truncated"));
    }
}
