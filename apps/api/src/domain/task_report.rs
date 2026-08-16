//! Project task report as a standalone document (.docx / .pdf).
//!
//! QA/product ask: hand someone outside the tool a readable snapshot of the
//! project — tasks grouped by status, with descriptions and subtasks — the
//! way the retro auto-summary reads, but as a file that survives email.
//!
//! Both writers are deliberately hand-rolled:
//!   * a .docx is just a zip with three XML parts — the `zip` crate is the
//!     only dependency this costs us, and Word/LibreOffice/Pages all open
//!     the minimal package happily (UTF-8 text comes along for free);
//!   * the .pdf writer emits plain page objects with the built-in Helvetica
//!     fonts. Base-14 fonts only speak Latin-1, so anything outside it is
//!     transliterated to '?' — the .docx is the faithful copy, and the modal
//!     says so. Embedding a Unicode font is a later, heavier decision.

use sqlx::PgPool;
use uuid::Uuid;

use crate::AppResult;

// ─── data ────────────────────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
struct ReportRow {
    id: Uuid,
    parent_task_id: Option<Uuid>,
    key: String,
    title: String,
    description: String,
    status: String,
    task_type: String,
    priority: String,
    column_name: Option<String>,
    assignee_handle: Option<String>,
    due_date: Option<chrono::NaiveDate>,
}

pub struct ReportTask {
    pub key: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub task_type: String,
    pub priority: String,
    pub column_name: Option<String>,
    pub assignee_handle: Option<String>,
    pub due_date: Option<chrono::NaiveDate>,
    pub subtasks: Vec<ReportTask>,
}

pub struct ReportData {
    pub project_key: String,
    pub project_name: String,
    pub generated_on: chrono::NaiveDate,
    /// (group label, tasks) in board order: todo → in progress → review → done.
    pub groups: Vec<(String, Vec<ReportTask>)>,
    pub total_tasks: usize,
}

const STATUS_ORDER: [(&str, &str); 4] = [
    ("todo", "To do"),
    ("in_progress", "In progress"),
    ("review", "In review"),
    ("done", "Done"),
];

pub async fn report_data(db: &PgPool, project_id: Uuid) -> AppResult<ReportData> {
    let (project_key, project_name): (String, String) =
        sqlx::query_as(r#"SELECT key, name FROM projects WHERE id = $1"#)
            .bind(project_id)
            .fetch_one(db)
            .await?;

    let rows: Vec<ReportRow> = sqlx::query_as(
        r#"
        SELECT t.id,
               t.parent_task_id,
               t.key,
               t.title,
               COALESCE(t.description, '')  AS description,
               t.status,
               t.type AS task_type,
               t.priority,
               bc.name    AS column_name,
               u.handle   AS assignee_handle,
               t.due_date
        FROM   tasks t
        LEFT JOIN board_columns bc ON bc.id = t.column_id
        LEFT JOIN users u          ON u.id = t.assignee_id
        WHERE  t.project_id = $1 AND t.deleted_at IS NULL
        ORDER  BY t.key
        "#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    let total_tasks = rows.len();

    // Split parents from subtasks, then hang children off their parents.
    let mut subtasks_of: std::collections::HashMap<Uuid, Vec<ReportTask>> =
        std::collections::HashMap::new();
    let mut parents: Vec<(Uuid, ReportTask)> = Vec::new();

    for r in rows {
        let task = ReportTask {
            key: r.key,
            title: r.title,
            description: r.description,
            status: r.status,
            task_type: r.task_type,
            priority: r.priority,
            column_name: r.column_name,
            assignee_handle: r.assignee_handle,
            due_date: r.due_date,
            subtasks: Vec::new(),
        };
        match r.parent_task_id {
            Some(pid) => subtasks_of.entry(pid).or_default().push(task),
            None => parents.push((r.id, task)),
        }
    }

    let mut groups: Vec<(String, Vec<ReportTask>)> = STATUS_ORDER
        .iter()
        .map(|(_, label)| ((*label).to_string(), Vec::new()))
        .collect();

    for (id, mut task) in parents {
        if let Some(subs) = subtasks_of.remove(&id) {
            task.subtasks = subs;
        }
        let idx = STATUS_ORDER
            .iter()
            .position(|(s, _)| *s == task.status)
            .unwrap_or(0);
        groups[idx].1.push(task);
    }

    // Orphaned subtasks (parent deleted) still deserve a line.
    for (_, mut orphans) in subtasks_of.drain() {
        for task in orphans.drain(..) {
            let idx = STATUS_ORDER
                .iter()
                .position(|(s, _)| *s == task.status)
                .unwrap_or(0);
            groups[idx].1.push(task);
        }
    }

    Ok(ReportData {
        project_key,
        project_name,
        generated_on: chrono::Utc::now().date_naive(),
        groups,
        total_tasks,
    })
}

fn meta_line(t: &ReportTask) -> String {
    let mut bits: Vec<String> = vec![t.task_type.clone(), t.priority.clone()];
    if let Some(c) = &t.column_name {
        bits.push(c.clone());
    }
    if let Some(a) = &t.assignee_handle {
        bits.push(format!("@{a}"));
    }
    if let Some(d) = &t.due_date {
        bits.push(format!("due {d}"));
    }
    bits.join(" · ")
}

// ─── docx ────────────────────────────────────────────────────────────────────

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// One paragraph. `style` is a paragraph style id from styles.xml; runs are
/// (text, bold) pairs so meta lines can mix weights without more styles.
fn docx_para(style: Option<&str>, runs: &[(&str, bool)]) -> String {
    let mut p = String::from("<w:p>");
    if let Some(s) = style {
        p.push_str(&format!("<w:pPr><w:pStyle w:val=\"{s}\"/></w:pPr>"));
    }
    for (text, bold) in runs {
        p.push_str("<w:r>");
        if *bold {
            p.push_str("<w:rPr><w:b/></w:rPr>");
        }
        // xml:space="preserve" keeps leading/trailing spaces in mixed runs.
        p.push_str(&format!(
            "<w:t xml:space=\"preserve\">{}</w:t>",
            xml_escape(text)
        ));
        p.push_str("</w:r>");
    }
    p.push_str("</w:p>");
    p
}

pub fn to_docx(data: &ReportData) -> AppResult<Vec<u8>> {
    let mut body = String::new();

    body.push_str(&docx_para(
        Some("Title"),
        &[(&format!("{} — task report", data.project_name), false)],
    ));
    body.push_str(&docx_para(
        None,
        &[(
            &format!(
                "{} · {} tasks · generated {}",
                data.project_key, data.total_tasks, data.generated_on
            ),
            false,
        )],
    ));

    for (label, tasks) in &data.groups {
        if tasks.is_empty() {
            continue;
        }
        body.push_str(&docx_para(
            Some("Heading1"),
            &[(&format!("{label} ({})", tasks.len()), false)],
        ));
        for t in tasks {
            body.push_str(&docx_para(
                Some("Heading2"),
                &[(&format!("{} — {}", t.key, t.title), false)],
            ));
            body.push_str(&docx_para(None, &[(&meta_line(t), true)]));
            for line in t.description.lines().filter(|l| !l.trim().is_empty()) {
                body.push_str(&docx_para(None, &[(line, false)]));
            }
            if !t.subtasks.is_empty() {
                body.push_str(&docx_para(None, &[("Subtasks:", true)]));
                for s in &t.subtasks {
                    let status_label = STATUS_ORDER
                        .iter()
                        .find(|(k, _)| *k == s.status)
                        .map(|(_, l)| *l)
                        .unwrap_or(s.status.as_str());
                    body.push_str(&docx_para(
                        None,
                        &[(
                            &format!("  • {} {} — {}", s.key, s.title, status_label),
                            false,
                        )],
                    ));
                    for line in s.description.lines().filter(|l| !l.trim().is_empty()) {
                        body.push_str(&docx_para(None, &[(&format!("      {line}"), false)]));
                    }
                }
            }
        }
    }

    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr/></w:body></w:document>"#
    );

    // Minimal style sheet: Title + two heading levels, sized so the report
    // reads like a document and not a wall of same-size lines.
    let styles = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:rPr><w:b/><w:sz w:val="48"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:spacing w:before="240" w:after="120"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:pPr><w:spacing w:before="180" w:after="60"/></w:pPr><w:rPr><w:b/><w:sz w:val="26"/></w:rPr></w:style>
</w:styles>"#;

    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
</Types>"#;

    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

    let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"#;

    Ok(store_zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
    ]))
}

// ─── zip container (STORED, no compression) ─────────────────────────────────
// A .docx is a zip archive; nothing in the spec requires deflate, and every
// consumer (Word, LibreOffice, Pages, unzip) reads STORED entries. Writing
// the container by hand keeps a whole compression crate out of the tree for
// what amounts to five small XML files.

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn store_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let n = files.len() as u16;

    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let name_bytes = name.as_bytes();

        // Local file header. Version 2.0, no flags, method 0 (stored),
        // DOS time/date zeroed — reproducible output, nobody reads it.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);

        // Matching central-directory record.
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }

    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    out.extend_from_slice(&central);

    // End of central directory.
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

// ─── pdf ─────────────────────────────────────────────────────────────────────

const PAGE_W: f32 = 595.0; // A4 portrait, points
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 50.0;

struct PdfLine {
    text: String,
    size: f32,
    bold: bool,
    indent: f32,
    gap_before: f32,
}

/// ASCII only. The content stream is written as raw bytes, so anything
/// multi-byte in UTF-8 (even Latin-1's é or ·) would render as mojibake in
/// a WinAnsi text stream. '?' is honest; the .docx is the faithful export.
fn pdf_sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect()
}

fn pdf_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

/// Crude-but-serviceable wrap: Helvetica averages ~0.5em per glyph.
fn wrap(text: &str, size: f32, indent: f32) -> Vec<String> {
    let usable = PAGE_W - 2.0 * MARGIN - indent;
    let max_chars = ((usable / (size * 0.5)) as usize).max(16);
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= max_chars {
                current.push(' ');
                current.push_str(word);
            } else {
                out.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

pub fn to_pdf(data: &ReportData) -> Vec<u8> {
    // 1. Flatten the report into styled lines.
    let mut lines: Vec<PdfLine> = Vec::new();
    let mut push = |text: &str, size: f32, bold: bool, indent: f32, gap: f32| {
        // The interpunct is this app's favourite separator; swap it for a
        // hyphen before the ASCII filter turns it into '?'.
        let clean = pdf_sanitize(&text.replace(" · ", " - "));
        for (i, l) in wrap(&clean, size, indent).into_iter().enumerate() {
            lines.push(PdfLine {
                text: l,
                size,
                bold,
                indent,
                gap_before: if i == 0 { gap } else { 0.0 },
            });
        }
    };

    push(
        &format!("{} — task report", data.project_name),
        16.0,
        true,
        0.0,
        0.0,
    );
    push(
        &format!(
            "{} · {} tasks · generated {}",
            data.project_key, data.total_tasks, data.generated_on
        ),
        9.5,
        false,
        0.0,
        4.0,
    );

    for (label, tasks) in &data.groups {
        if tasks.is_empty() {
            continue;
        }
        push(&format!("{label} ({})", tasks.len()), 13.0, true, 0.0, 16.0);
        for t in tasks {
            push(&format!("{} — {}", t.key, t.title), 11.0, true, 0.0, 10.0);
            push(&meta_line(t), 8.5, false, 0.0, 2.0);
            for line in t.description.lines().filter(|l| !l.trim().is_empty()) {
                push(line, 9.5, false, 8.0, 2.0);
            }
            if !t.subtasks.is_empty() {
                push("Subtasks:", 9.5, true, 8.0, 4.0);
                for s in &t.subtasks {
                    let status_label = STATUS_ORDER
                        .iter()
                        .find(|(k, _)| *k == s.status)
                        .map(|(_, l)| *l)
                        .unwrap_or(s.status.as_str());
                    push(
                        &format!("- {} {} — {}", s.key, s.title, status_label),
                        9.5,
                        false,
                        16.0,
                        2.0,
                    );
                }
            }
        }
    }

    // 2. Lay lines onto pages.
    let mut pages: Vec<String> = Vec::new();
    let mut content = String::new();
    let mut y = PAGE_H - MARGIN;
    for line in &lines {
        let advance = line.gap_before + line.size * 1.35;
        if y - advance < MARGIN {
            pages.push(std::mem::take(&mut content));
            y = PAGE_H - MARGIN;
        }
        y -= advance;
        if line.text.is_empty() {
            continue;
        }
        let font = if line.bold { "F2" } else { "F1" };
        content.push_str(&format!(
            "BT /{font} {size} Tf {x:.1} {y:.1} Td ({text}) Tj ET\n",
            size = line.size,
            x = MARGIN + line.indent,
            text = pdf_escape(&line.text),
        ));
    }
    pages.push(content);

    // 3. Serialize the object graph: catalog(1) → pages(2) → [page, stream]×n,
    //    then the two fonts. Object numbers are computed positionally.
    let n_pages = pages.len();
    let font1_id = 3 + 2 * n_pages;
    let font2_id = font1_id + 1;

    let mut objects: Vec<String> = Vec::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
    let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", 3 + 2 * i)).collect();
    objects.push(format!(
        "<< /Type /Pages /Count {n_pages} /Kids [{}] >>",
        kids.join(" ")
    ));
    for (i, page_content) in pages.iter().enumerate() {
        let page_id = 3 + 2 * i;
        let stream_id = page_id + 1;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Contents {stream_id} 0 R /Resources << /Font << /F1 {font1_id} 0 R /F2 {font2_id} 0 R >> >> >>"
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}endstream",
            page_content.len(),
            page_content
        ));
    }
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_string(),
    );
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_string(),
    );

    let mut out = String::from("%PDF-1.4\n");
    let mut offsets: Vec<usize> = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, obj));
    }
    let xref_at = out.len();
    out.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
    out.push_str("0000000000 65535 f \n");
    for off in &offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF",
        objects.len() + 1
    ));
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ReportData {
        ReportData {
            project_key: "TR".into(),
            project_name: "Test réport".into(),
            generated_on: chrono::NaiveDate::from_ymd_opt(2026, 8, 12).unwrap(),
            groups: vec![
                (
                    "To do".into(),
                    vec![ReportTask {
                        key: "TR-1".into(),
                        title: "Ship the (thing)".into(),
                        description: "Line one.\n\nLine two with <tags> & \"quotes\".".into(),
                        status: "todo".into(),
                        task_type: "feature".into(),
                        priority: "p2".into(),
                        column_name: Some("To do".into()),
                        assignee_handle: Some("sam".into()),
                        due_date: None,
                        subtasks: vec![ReportTask {
                            key: "TR-2".into(),
                            title: "Subtask".into(),
                            description: String::new(),
                            status: "done".into(),
                            task_type: "chore".into(),
                            priority: "p3".into(),
                            column_name: None,
                            assignee_handle: None,
                            due_date: None,
                            subtasks: vec![],
                        }],
                    }],
                ),
                ("Done".into(), vec![]),
            ],
            total_tasks: 2,
        }
    }

    #[test]
    fn crc32_reference_vector() {
        // The canonical IEEE 802.3 check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn docx_is_a_wellformed_zip() {
        let bytes = to_docx(&sample()).unwrap();
        // Local header magic…
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        // …EOCD magic present in the tail…
        let eocd = &bytes[bytes.len() - 22..bytes.len() - 18];
        assert_eq!(eocd, b"PK\x05\x06");
        // …and the document part (with escaped content) made it in.
        let hay = String::from_utf8_lossy(&bytes);
        assert!(hay.contains("word/document.xml"));
        assert!(hay.contains("&lt;tags&gt; &amp; &quot;quotes&quot;"));
        assert!(hay.contains("TR-2 Subtask — Done"));
    }

    #[test]
    fn pdf_has_shape_and_ascii_only_text() {
        let bytes = to_pdf(&sample());
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("%PDF-1.4"));
        assert!(s.trim_end().ends_with("%%EOF"));
        // Non-ASCII input is transliterated, never emitted as multi-byte
        // UTF-8 (which a WinAnsi stream would render as mojibake).
        assert!(!s.contains('·'));
        assert!(
            s.contains("Test r?port"),
            "é must become ?, not UTF-8 bytes"
        );
        assert!(s.contains("(TR-1"));
        // Balanced object graph: every obj has an endobj.
        assert_eq!(s.matches(" 0 obj").count(), s.matches("endobj").count());
    }

    #[test]
    fn escaped_parens_in_pdf_strings() {
        let bytes = to_pdf(&sample());
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("Ship the \\(thing\\)"));
    }
}
