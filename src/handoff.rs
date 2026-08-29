//! The page a browser lands on at the end of `nuage login`.
//!
//! Every Facile CLI serves this page from its own binary on `127.0.0.1`, and
//! every one of them used to draw its own. The markup is therefore not written
//! here: `handoff.html.tmpl` is a byte-for-byte copy of the file the Go half of
//! the suite renders (`Mycelium/internal/server/handoff.html.tmpl`, adopted in
//! `porte/internal/handoff`), so `diff` is all it takes to prove the pages have
//! not drifted. Rewriting the markup in Rust would have made that impossible to
//! check and guaranteed the drift within a release.
//!
//! Nuage cannot import `porte/loopback` the way the Go CLIs now do, so it
//! carries the template and renders it here instead. What it must not do is
//! carry a *different* page.

/// The markup, verbatim. Keeping it as a file rather than a string literal is
/// what lets it be diffed against the suite's copy without reading past Rust
/// quoting, and what keeps a stray edit here visible as a one-line diff.
const MARKUP: &str = include_str!("handoff.html.tmpl");

/// The tool the page names. Once the browser is on `127.0.0.1` it has left
/// Nuage's domain behind and the address bar proves nothing, so this is all a
/// person has to tell this login from any other local process that asked them
/// to sign in.
const APP_NAME: &str = "Nuage";

/// The last line of a page that ends the login, and of one that does not. A
/// refusal must never send the reader back to the terminal: a refused callback
/// leaves the login open and the terminal really is still waiting, so telling
/// them otherwise sends them to restart something that is working.
const HINT_CLOSE: &str = "You can close this tab and go back to your terminal.";
const HINT_WAITING: &str = "The login started in your terminal is still waiting.";

/// The template's opening and closing actions. The renderer understands these
/// two and the `{{.Field}}` substitutions below, which is every action the
/// template uses.
const END: &str = "{{end}}";

/// One rendering of the page.
///
/// The fields are `&'static str` rather than `&str` on purpose. The Go side
/// renders through `html/template` because its app name arrives from
/// configuration; nothing on this page arrives from anywhere but the constants
/// below, so there is nothing to escape. Widening these to a runtime string
/// means adding the escaping the Go side has, and the signature is what makes
/// that a deliberate change rather than an accident.
pub struct Page {
    heading: &'static str,
    body: &'static str,
    hint: &'static str,
    warn: bool,
}

/// The code landed and the login is over.
pub const SIGNED_IN: Page = Page {
    heading: "Signed in",
    body: "Nuage has your login.",
    hint: HINT_CLOSE,
    warn: false,
};

/// A callback whose nonce is not this login's. The login stays open, which is
/// why the hint says so.
pub const NOT_THIS_LOGIN: Page = Page {
    heading: "That is not this login",
    body: "This callback does not belong to the login that is waiting.",
    hint: HINT_WAITING,
    warn: true,
};

/// Anything else the browser asked for, `/favicon.ico` above all. It is not an
/// error the person did anything about, and the login is still open.
pub const NO_CODE: Page = Page {
    heading: "No login code",
    body: "This callback carries no login code.",
    hint: HINT_WAITING,
    warn: true,
};

impl Page {
    /// Fills the template in.
    ///
    /// `LogoURL` and `Code` are always dropped: this server is a CLI binary on
    /// `127.0.0.1`, so a page that fetches an image is a page that hangs on the
    /// laptop whose network is the thing being fixed, and the one-time code
    /// never reaches this page at all. It rides the redirect straight into the
    /// terminal, which is the difference between this surface and the paste
    /// page the server renders for a CLI with no listener.
    pub fn render(&self) -> String {
        let markup = resolve(MARKUP, "{{if .LogoURL}}", false);
        let markup = resolve(&markup, "{{if .Code}}", false);
        let markup = resolve(&markup, "{{if .Warn}}", self.warn);
        markup
            .replace("{{.AppName}}", APP_NAME)
            .replace("{{.Heading}}", self.heading)
            .replace("{{.Body}}", self.body)
            .replace("{{.Hint}}", self.hint)
    }
}

/// Keeps or drops every `open`-to-`{{end}}` span in `markup`.
///
/// This is the whole of the template language the page needs, and deliberately
/// not one action more. A template that grows a nested conditional or a range
/// would come out of here with its actions still in it, which the test below
/// fails on rather than serving `{{if .Something}}` to a browser.
fn resolve(markup: &str, open: &str, keep: bool) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(start) = rest.find(open) {
        out.push_str(&rest[..start]);
        let inner = &rest[start + open.len()..];
        let Some(close) = inner.find(END) else {
            out.push_str(inner);
            return out;
        };
        if keep {
            out.push_str(&inner[..close]);
        }
        rest = &inner[close + END.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // An unresolved action reaches the browser as literal text, which is how a
    // template that grew a feature this renderer does not know would ship: no
    // error, no warning, just `{{if .Something}}` printed on the page that ends
    // a login.
    #[test]
    fn every_action_in_the_template_is_resolved() {
        for page in [&SIGNED_IN, &NOT_THIS_LOGIN, &NO_CODE] {
            let rendered = page.render();
            assert!(
                !rendered.contains("{{"),
                "an unresolved action survived rendering:\n{rendered}"
            );
        }
    }

    // The page has to name the tool and say what happened. The address bar says
    // 127.0.0.1 and proves nothing about who asked for this login.
    #[test]
    fn the_success_page_names_nuage_and_sends_the_reader_back() {
        let rendered = SIGNED_IN.render();
        assert!(rendered.contains("<span>Nuage</span>"));
        assert!(rendered.contains("<h1>Signed in</h1>"));
        assert!(rendered.contains("Nuage has your login."));
        assert!(rendered.contains(HINT_CLOSE));
        assert!(!rendered.contains("class=\"warn\""));
    }

    // A refusal is the same page with the body recoloured, and it must not tell
    // the reader to go back to the terminal: the login it refused is still
    // waiting there for the callback that does belong to it.
    #[test]
    fn a_refusal_warns_and_says_the_login_is_still_open() {
        for page in [&NOT_THIS_LOGIN, &NO_CODE] {
            let rendered = page.render();
            assert!(rendered.contains("<p class=\"warn\">"), "{rendered}");
            assert!(rendered.contains(HINT_WAITING), "{rendered}");
            assert!(!rendered.contains(HINT_CLOSE), "{rendered}");
        }
    }

    // The loopback page carries no image and no code. The image would hang on
    // the machine whose network is broken, and the code is already in the
    // terminal that started the login.
    #[test]
    fn the_page_fetches_nothing_and_prints_no_code() {
        let rendered = SIGNED_IN.render();
        assert!(!rendered.contains("<img"));
        assert!(!rendered.contains("<output"));
        assert!(!rendered.contains("<script"));
    }
}
