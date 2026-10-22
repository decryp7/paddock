//! Pages that need the user: logins, CAPTCHAs, interstitial "checking your browser" pages and consent walls.
//! Pure heuristics over what the page shows. A challenge is handed to the user, never solved.

/// What kind of wall a page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Challenge {
    /// A cookie or consent wall.
    Consent,
    /// A CAPTCHA, Cloudflare-style interstitial or "verify you are human" check.
    HumanCheck,
    /// A login form instead of the content.
    LoginWall,
}

impl Challenge {
    /// A sentence for the "needs you" state.
    pub fn reason(self) -> &'static str {
        match self {
            Challenge::Consent => "the page asks for cookie consent before it shows its content",
            Challenge::HumanCheck => "the page asks to prove you are human (captcha or browser check)",
            Challenge::LoginWall => "the page asks you to sign in",
        }
    }
}

/// What the renderer observed.
#[derive(Debug, Clone, Copy)]
pub struct Signals<'a> {
    /// Current address.
    pub url: &'a str,
    /// Document title.
    pub title: &'a str,
    /// A bounded excerpt of the visible text.
    pub text: &'a str,
    /// A password input exists.
    pub has_password_field: bool,
    /// The element the watch waits for is present (a login form only counts as a wall when it is not).
    pub target_present: bool,
}

const CONSENT_HOSTS: &[&str] = &["consent.google.", "consent.youtube."];
const HUMAN_URL: &[&str] = &["/sorry/", "captcha", "cdn-cgi/challenge"];
const HUMAN_TITLE: &[&str] = &["just a moment", "attention required", "access denied", "are you a robot"];
const HUMAN_TEXT: &[&str] = &[
    "verify you are human",
    "verify you're human",
    "are you a robot",
    "i'm not a robot",
    "unusual traffic",
    "not a robot",
    "complete the security check",
    "checking your browser",
    "enable javascript and cookies to continue",
    "press & hold",
    "captcha",
];
const CONSENT_TEXT: &[&str] = &["before you continue", "we use cookies", "accept all", "reject all"];
const LOGIN_URL: &[&str] = &["/login", "/signin", "/sign-in", "/sso", "/auth/", "/oauth", "/accounts/login"];
const LOGIN_TEXT: &[&str] = &["sign in", "log in", "login", "password"];

/// The challenge this page shows, if any. Only the first 4000 characters of the text are looked at.
pub fn detect(s: &Signals<'_>) -> Option<Challenge> {
    let url = s.url.to_ascii_lowercase();
    let title = s.title.to_lowercase();
    let text: String = s.text.chars().take(4000).collect::<String>().to_lowercase();
    let head = format!("{title} {text}");
    if HUMAN_URL.iter().any(|u| url.contains(u))
        || (HUMAN_TITLE.iter().any(|t| title.contains(t)) && !s.target_present)
        || HUMAN_TEXT.iter().any(|t| head.contains(t))
    {
        return Some(Challenge::HumanCheck);
    }
    let host = url.split("//").nth(1).unwrap_or("");
    if CONSENT_HOSTS.iter().any(|h| host.starts_with(h)) {
        return Some(Challenge::Consent);
    }
    if !s.target_present {
        if CONSENT_TEXT.iter().filter(|t| head.contains(*t)).count() >= 2 && s.text.chars().count() < 3000 {
            return Some(Challenge::Consent);
        }
        let path = host.split_once('/').map(|x| x.1).unwrap_or("");
        let login_url = LOGIN_URL.iter().any(|u| format!("/{path}").contains(u));
        let login_text = LOGIN_TEXT.iter().filter(|t| head.contains(*t)).count() >= 2;
        if s.has_password_field && (login_url || login_text) {
            return Some(Challenge::LoginWall);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig<'a>(url: &'a str, title: &'a str, text: &'a str) -> Signals<'a> {
        Signals {
            url,
            title,
            text,
            has_password_field: false,
            target_present: false,
        }
    }

    #[test]
    fn captcha_and_cloudflare_pages() {
        assert_eq!(detect(&sig("https://g.test/sorry/index", "", "")), Some(Challenge::HumanCheck));
        assert_eq!(
            detect(&sig(
                "https://x.test/",
                "Just a moment...",
                "Checking your browser before accessing x.test"
            )),
            Some(Challenge::HumanCheck)
        );
        assert_eq!(
            detect(&sig("https://x.test/", "", "Our systems have detected unusual traffic")),
            Some(Challenge::HumanCheck)
        );
    }

    #[test]
    fn consent_pages() {
        assert_eq!(
            detect(&sig("https://consent.google.com/ml?x=1", "Before you continue", "")),
            Some(Challenge::Consent)
        );
        assert_eq!(
            detect(&sig("https://a.test/", "", "We use cookies. Accept all  Reject all")),
            Some(Challenge::Consent)
        );
    }

    #[test]
    fn login_wall_needs_a_password_field_and_a_missing_target() {
        let mut s = sig("https://a.test/login?next=/r", "Sign in", "Email Password Log in");
        assert_eq!(detect(&s), None);
        s.has_password_field = true;
        assert_eq!(detect(&s), Some(Challenge::LoginWall));
        s.target_present = true;
        assert_eq!(detect(&s), None);
        let t = Signals {
            has_password_field: true,
            ..sig("https://a.test/releases", "Releases", "v1.2")
        };
        assert_eq!(detect(&t), None);
    }

    #[test]
    fn ordinary_pages_are_not_challenges() {
        assert_eq!(
            detect(&sig("https://coding.test/challenge/42", "Daily challenge", "Solve it")),
            None
        );
        assert_eq!(
            detect(&sig("https://docs.test/a", "Null reference", "How to fix it. We use cookies.")),
            None
        );
        let long = "word ".repeat(2000) + " accept all reject all we use cookies";
        assert_eq!(detect(&sig("https://docs.test/a", "t", &long)), None);
    }

    #[test]
    fn every_kind_has_a_reason() {
        for c in [Challenge::Consent, Challenge::HumanCheck, Challenge::LoginWall] {
            assert!(!c.reason().is_empty());
        }
    }
}
