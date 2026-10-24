// PII scrubbing for captured page text, and for the model's summary before it
// can be shown to a stranger.
use regex::Regex;
use std::sync::LazyLock;

pub const MAX_CHARS: usize = 2000;

static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[a-z0-9._%+-]+@[a-z0-9-]+(?:\.[a-z0-9-]+)*\.[a-z]{2,}").unwrap()
});
// The regex crate has no lookbehind, so the character before the number is
// captured and written back by the replacement.
static PHONE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|[^\d])(?:\+\d{1,3}[ .-]?)?(?:\(\d{3}\)|\d{3})[ .-]?\d{3}[ .-]?\d{4}\b").unwrap()
});
static LONG_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d(?:[ -]?\d){7,}").unwrap());

/// Emails, phone numbers and runs of 8+ digits become placeholders; whitespace
/// is collapsed and the result is cut to MAX_CHARS characters.
pub fn scrub(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = EMAIL.replace_all(&collapsed, "[email]");
    let text = PHONE.replace_all(&text, "${1}[phone]");
    let text = LONG_NUMBER.replace_all(&text, "[number]");
    text.chars().take(MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_emails() {
        assert_eq!(
            scrub("Email jane.doe+work@mail.example.co.uk or BOB@EXAMPLE.COM today"),
            "Email [email] or [email] today"
        );
    }

    #[test]
    fn replaces_phone_numbers() {
        assert_eq!(
            scrub("Call (415) 555-1234, +1 415.555.1234 or 4155551234."),
            "Call [phone], [phone] or [phone]."
        );
    }

    #[test]
    fn replaces_long_digit_runs() {
        assert_eq!(scrub("Card 4111 1111 1111 1111 exp"), "Card [number] exp");
        assert_eq!(scrub("SSN 123-45-6789, order #123456789"), "SSN [number], order #[number]");
    }

    #[test]
    fn leaves_normal_text_alone() {
        let text = "207. Course Schedule - LeetCode, CS 61B at 12:30, 2 <= n <= 10^5, $1,200 rent";
        assert_eq!(scrub(text), text);
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(scrub("  graph\n\n problems \t BFS "), "graph problems BFS");
    }

    #[test]
    fn truncates_on_char_boundaries() {
        let scrubbed = scrub(&"é".repeat(MAX_CHARS + 500));
        assert_eq!(scrubbed.chars().count(), MAX_CHARS);
    }
}
