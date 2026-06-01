//! Minimal PDF generator. ~80 lines, no external crate.
//!
//! Why hand-roll: the alternatives (`printpdf`, `genpdf`) pull in ~3-5MB of
//! compile-time weight plus their own font assets. For "render a simple
//! payroll report" we only need text in the standard Type-1 Helvetica that
//! every PDF reader ships with — there's no font embedding to do.
//!
//! Output is PDF 1.4, US Letter (612 × 792 pt), single page, ASCII-only.
//! Anything non-ASCII in the input is escaped to `?` to avoid encoding
//! pitfalls (we can swap to a proper font with Unicode tables in M10 if
//! anyone cares).

pub struct PdfBuilder {
    page_lines: Vec<Line>,
}

struct Line {
    x: f32,
    y: f32,
    font_size: f32,
    text: String,
}

const PAGE_W: f32 = 612.0;
const PAGE_H: f32 = 792.0;

impl PdfBuilder {
    pub fn new() -> Self {
        Self { page_lines: Vec::new() }
    }

    /// Place text at `(x, y)` measured in PDF points from the bottom-left.
    pub fn text(&mut self, x: f32, y: f32, font_size: f32, text: &str) -> &mut Self {
        self.page_lines.push(Line {
            x,
            y,
            font_size,
            text: escape(text),
        });
        self
    }

    /// Convenience for top-aligned layout: y measured from the top.
    pub fn text_top(&mut self, x: f32, y_from_top: f32, font_size: f32, text: &str) -> &mut Self {
        self.text(x, PAGE_H - y_from_top, font_size, text)
    }

    /// Build the final PDF bytes.
    pub fn finish(&self) -> Vec<u8> {
        // Content stream: text-show commands.
        let mut stream = String::new();
        for line in &self.page_lines {
            stream.push_str(&format!(
                "BT /F1 {} Tf {} {} Td ({}) Tj ET\n",
                line.font_size, line.x, line.y, line.text
            ));
        }
        let stream_bytes = stream.as_bytes();

        // Build five PDF indirect objects. Offsets are recorded as we write.
        let mut out: Vec<u8> = Vec::with_capacity(1024 + stream_bytes.len());
        let mut offsets: Vec<usize> = Vec::with_capacity(6);
        out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

        // 1: Catalog
        offsets.push(out.len());
        out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

        // 2: Pages
        offsets.push(out.len());
        out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");

        // 3: Page
        offsets.push(out.len());
        let page = format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n",
        );
        out.extend_from_slice(page.as_bytes());

        // 4: Content stream
        offsets.push(out.len());
        let header = format!("4 0 obj\n<< /Length {} >>\nstream\n", stream_bytes.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(stream_bytes);
        out.extend_from_slice(b"endstream\nendobj\n");

        // 5: Font (built-in Helvetica)
        offsets.push(out.len());
        out.extend_from_slice(b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n");

        // Cross-reference table.
        let xref_start = out.len();
        out.extend_from_slice(b"xref\n0 6\n");
        out.extend_from_slice(b"0000000000 65535 f \n");
        for off in &offsets {
            out.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
        }

        // Trailer.
        let trailer = format!(
            "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            xref_start
        );
        out.extend_from_slice(trailer.as_bytes());
        out
    }
}

/// Escape PDF text literals: parens and backslashes must be quoted, control
/// chars become spaces, non-ASCII becomes `?`. Cheap and good enough for the
/// payroll report's needs.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_ascii_graphic() || c == ' ' => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_valid_header_and_footer() {
        let mut b = PdfBuilder::new();
        b.text(50.0, 700.0, 12.0, "Hello, world.");
        let bytes = b.finish();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        // Must contain the text in some form (post-escape). The PDF has a
        // binary marker comment, so it isn't valid UTF-8 as a whole — use a
        // lossy decode that keeps the ASCII content stream intact.
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("(Hello, world.) Tj"));
    }

    #[test]
    fn escapes_parens_and_strips_non_ascii() {
        let mut b = PdfBuilder::new();
        b.text(0.0, 0.0, 10.0, "(a)\\ über");
        let bytes = b.finish();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains(r"\(a\)\\ ?ber"));
    }
}
