//! Pull indexable text out of a document, whatever it is wrapped in.
//!
//! All local, extraction only: PDF via `pdf-extract`, Excel via `calamine`,
//! Word and PowerPoint by reading the XML inside their ZIP containers.
//! Rendering — actually *looking* at a page — stays with the CLI's own Read
//! tool; this module exists so the semantic index can see inside formats
//! Grep cannot.
//!
//! Callers run this under `spawn_blocking`: PDF parsing is CPU work, and a
//! panic inside a parser (malformed files find them) becomes a JoinError
//! there instead of taking the server down.

use std::io::Cursor;

/// What extraction produced.
#[derive(Debug)]
pub enum Extracted {
    Text(String),
    /// Not an error: the file stays in the folder where Read can open it —
    /// it just cannot join the semantic index. The reason is user-facing.
    Unsupported(&'static str),
}

/// Extracted text beyond this is cut. A million characters is ~800 chunks —
/// generous for any real document, and a ceiling on what one pathological
/// file can demand of the embedder.
pub const MAX_EXTRACT_CHARS: usize = 1_000_000;

/// Extract by extension. The extension was already vetted at upload for
/// files that came through the app; files dropped into the folder by hand
/// get the same dispatch and honest `Unsupported` answers.
pub fn extract(rel_path: &str, bytes: &[u8]) -> anyhow::Result<Extracted> {
    let ext = rel_path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let text = match ext.as_str() {
        "pdf" => pdf(bytes)?,
        "docx" => docx(bytes)?,
        "pptx" => pptx(bytes)?,
        "xlsx" | "xlsm" | "xls" | "ods" => spreadsheet(bytes)?,
        // Legacy binary Office formats have no pure-Rust reader worth the
        // weight; saying so beats a parse error dressed up as a failure.
        "doc" | "ppt" => {
            return Ok(Extracted::Unsupported(
                "legacy binary format — save it as .docx/.pptx to index it",
            ))
        }
        // Everything else is text if it looks like text — csv included,
        // which indexes verbatim: headers and cells are exactly what a
        // question about the data will use.
        _ => {
            if !looks_like_text(bytes) {
                return Ok(Extracted::Unsupported(
                    "not a text file — the assistant can still Read it in the folder",
                ));
            }
            String::from_utf8_lossy(bytes).into_owned()
        }
    };
    Ok(Extracted::Text(clip(text)))
}

fn clip(mut text: String) -> String {
    if text.chars().count() > MAX_EXTRACT_CHARS {
        let cut = text
            .char_indices()
            .nth(MAX_EXTRACT_CHARS)
            .map(|(i, _)| i)
            .unwrap_or(text.len());
        text.truncate(cut);
        text.push_str("\n\n[truncated — the document continues beyond the index ceiling]");
    }
    text
}

/// Valid UTF-8, no NUL, few control characters. The heuristic `routes/kb.rs`
/// documents: valid UTF-8 alone is not enough — an ELF header passes it.
pub fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    let Ok(s) = std::str::from_utf8(bytes) else {
        return false;
    };
    let control = s
        .chars()
        .filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        .count();
    control * 100 <= s.chars().count().max(1)
}

fn pdf(bytes: &[u8]) -> anyhow::Result<String> {
    let text = pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| anyhow::anyhow!("could not read the PDF: {e}"))?;
    if text.trim().is_empty() {
        // A scanned PDF has pages and no text layer. Extraction "succeeded"
        // at extracting nothing, which is worth saying plainly.
        anyhow::bail!("the PDF has no text layer (a scan?) — nothing to index");
    }
    Ok(text)
}

/// Word: one XML document, text in `<w:t>`, paragraphs in `<w:p>`.
fn docx(bytes: &[u8]) -> anyhow::Result<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("not a .docx (zip) file: {e}"))?;
    let mut xml = String::new();
    {
        use std::io::Read;
        let mut f = archive
            .by_name("word/document.xml")
            .map_err(|_| anyhow::anyhow!("no word/document.xml inside — not a Word file?"))?;
        f.read_to_string(&mut xml)?;
    }
    Ok(xml_text(&xml, "w:t", "w:p"))
}

/// PowerPoint: one XML per slide, text in `<a:t>`, paragraphs in `<a:p>`.
/// Slides are numbered files; sorted so the text reads in deck order.
fn pptx(bytes: &[u8]) -> anyhow::Result<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("not a .pptx (zip) file: {e}"))?;
    let mut slides: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let name = archive.by_index(i).ok()?.name().to_string();
            (name.starts_with("ppt/slides/slide") && name.ends_with(".xml")).then_some(name)
        })
        .collect();
    if slides.is_empty() {
        anyhow::bail!("no slides inside — not a PowerPoint file?");
    }
    // slide2.xml must not sort after slide10.xml: order by the number.
    slides.sort_by_key(|n| {
        n.trim_start_matches("ppt/slides/slide")
            .trim_end_matches(".xml")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });
    let mut out = String::new();
    for (i, name) in slides.iter().enumerate() {
        use std::io::Read;
        let mut xml = String::new();
        archive.by_name(name)?.read_to_string(&mut xml)?;
        out.push_str(&format!("## Slide {}\n\n", i + 1));
        out.push_str(&xml_text(&xml, "a:t", "a:p"));
        out.push_str("\n\n");
    }
    Ok(out)
}

/// Text content of `<{text_tag}>` elements, with a newline per `{para_tag}`.
fn xml_text(xml: &str, text_tag: &str, para_tag: &str) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == text_tag.as_bytes() => in_text = true,
            Ok(Event::End(e)) => {
                let name = e.name();
                if name.as_ref() == text_tag.as_bytes() {
                    in_text = false;
                } else if name.as_ref() == para_tag.as_bytes() {
                    out.push('\n');
                }
            }
            Ok(Event::Text(t)) if in_text => {
                if let Ok(s) = t.decode() {
                    out.push_str(&s);
                }
            }
            Ok(Event::Eof) => break,
            // Malformed XML inside an otherwise-valid zip: keep what we got.
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out.trim().to_string()
}

/// Excel and friends: every sheet, rows as tab-joined lines. Tables read
/// fine to both the chunker and the model this way, and the sheet name is
/// the context a row needs.
fn spreadsheet(bytes: &[u8]) -> anyhow::Result<String> {
    use calamine::{Data, Reader};
    let mut workbook = calamine::open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("could not read the spreadsheet: {e}"))?;
    let mut out = String::new();
    let sheets = workbook.sheet_names().to_vec();
    for sheet in sheets {
        let Ok(range) = workbook.worksheet_range(&sheet) else {
            continue;
        };
        if range.is_empty() {
            continue;
        }
        out.push_str(&format!("## Sheet: {sheet}\n\n"));
        for row in range.rows() {
            let line: Vec<String> = row
                .iter()
                .map(|c| match c {
                    Data::Empty => String::new(),
                    other => other.to_string(),
                })
                .collect();
            // A fully empty row is layout, not data.
            if line.iter().all(|c| c.is_empty()) {
                continue;
            }
            out.push_str(&line.join("\t"));
            out.push('\n');
        }
        out.push('\n');
    }
    if out.trim().is_empty() {
        anyhow::bail!("the spreadsheet has no cells with content");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A minimal but valid ZIP with the given members.
    fn zip_of(members: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for (name, content) in members {
                w.start_file(*name, opts).unwrap();
                w.write_all(content.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn plain_text_and_csv_pass_through() {
        let Extracted::Text(t) = extract("notes.md", b"# Title\n\nBody.").unwrap() else {
            panic!("markdown is text");
        };
        assert_eq!(t, "# Title\n\nBody.");
        let Extracted::Text(t) = extract("data.csv", b"name,amount\ncoffee,4.50\n").unwrap() else {
            panic!("csv is text");
        };
        assert!(t.contains("coffee,4.50"));
    }

    #[test]
    fn a_binary_blob_is_unsupported_not_an_error() {
        match extract("blob.bin", &[0x7f, b'E', b'L', b'F', 0, 0]).unwrap() {
            Extracted::Unsupported(reason) => assert!(reason.contains("Read")),
            Extracted::Text(_) => panic!("an ELF header is not text"),
        }
    }

    #[test]
    fn word_text_comes_out_of_the_xml() {
        let doc = zip_of(&[(
            "word/document.xml",
            r#"<?xml version="1.0"?><w:document><w:body>
                <w:p><w:r><w:t>The deploy window</w:t></w:r><w:r><w:t> is Tuesday.</w:t></w:r></w:p>
                <w:p><w:r><w:t>Second paragraph.</w:t></w:r></w:p>
            </w:body></w:document>"#,
        )]);
        let Extracted::Text(t) = extract("handbook.docx", &doc).unwrap() else {
            panic!("docx should extract");
        };
        assert!(t.contains("The deploy window is Tuesday."), "{t:?}");
        assert!(t.contains("Second paragraph."));
        // Paragraphs are separated, not run together.
        assert!(t.find("Tuesday.").unwrap() < t.find("Second").unwrap());
        assert!(t.contains('\n'));
    }

    #[test]
    fn a_zip_that_is_not_word_says_so() {
        let notdoc = zip_of(&[("random.txt", "hello")]);
        let err = extract("fake.docx", &notdoc).unwrap_err();
        assert!(err.to_string().contains("not a Word file"), "{err}");
    }

    #[test]
    fn slides_extract_in_deck_order_not_string_order() {
        // slide10 vs slide2: string order would put 10 first.
        let deck = zip_of(&[
            (
                "ppt/slides/slide10.xml",
                r#"<p:sld><a:p><a:r><a:t>Tenth slide</a:t></a:r></a:p></p:sld>"#,
            ),
            (
                "ppt/slides/slide2.xml",
                r#"<p:sld><a:p><a:r><a:t>Second slide</a:t></a:r></a:p></p:sld>"#,
            ),
        ]);
        let Extracted::Text(t) = extract("deck.pptx", &deck).unwrap() else {
            panic!("pptx should extract");
        };
        assert!(
            t.find("Second slide").unwrap() < t.find("Tenth slide").unwrap(),
            "{t:?}"
        );
        assert!(t.contains("## Slide 1"));
    }

    #[test]
    fn a_spreadsheet_becomes_tab_separated_lines_per_sheet() {
        // A minimal xlsx calamine accepts: workbook + one sheet of inline
        // strings. Hand-built so the test stays pure.
        let xlsx = zip_of(&[
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>item</t></is></c><c r="B1" t="inlineStr"><is><t>cost</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>laptop stand</t></is></c><c r="B2"><v>51.25</v></c></row></sheetData></worksheet>"#,
            ),
        ]);
        let Extracted::Text(t) = extract("budget.xlsx", &xlsx).unwrap() else {
            panic!("xlsx should extract");
        };
        assert!(t.contains("## Sheet: Budget"), "{t:?}");
        assert!(t.contains("item\tcost"));
        assert!(t.contains("laptop stand\t51.25"));
    }

    #[test]
    fn a_minimal_pdf_extracts_its_text() {
        // A smallest-viable PDF with one text object, built with a real xref
        // table — the parser requires one, and computing the offsets here is
        // what keeps this a fixture-free test.
        let body = "BT /F1 12 Tf 72 712 Td (Deploys are frozen on Fridays.) Tj ET";
        let objects = [
            "1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n".to_string(),
            "2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n".to_string(),
            "3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj\n".to_string(),
            format!("4 0 obj << /Length {} >> stream\n{}\nendstream endobj\n", body.len(), body),
            "5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = vec![];
        for obj in &objects {
            offsets.push(pdf.len());
            pdf.push_str(obj);
        }
        let xref_at = pdf.len();
        pdf.push_str(&format!("xref\n0 6\n{:010} 65535 f \n", 0));
        for off in &offsets {
            pdf.push_str(&format!("{off:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer << /Root 1 0 R /Size 6 >>\nstartxref\n{xref_at}\n%%EOF\n"
        ));
        let Extracted::Text(t) = extract("policy.pdf", pdf.as_bytes()).unwrap() else {
            panic!("the minimal pdf should extract");
        };
        assert!(t.contains("Deploys are frozen on Fridays."), "{t:?}");
    }

    #[test]
    fn a_broken_pdf_is_an_error_with_a_reason_not_a_crash() {
        let err = extract("junk.pdf", b"%PDF-1.4\ngarbage").unwrap_err();
        assert!(err.to_string().contains("PDF"), "{err}");
    }

    #[test]
    fn legacy_office_formats_get_an_honest_answer() {
        match extract("old.doc", b"\xd0\xcf\x11\xe0whatever").unwrap() {
            Extracted::Unsupported(reason) => assert!(reason.contains(".docx")),
            _ => panic!(".doc has no reader"),
        }
    }

    #[test]
    fn extraction_is_capped() {
        let huge = "x".repeat(MAX_EXTRACT_CHARS + 50_000);
        let Extracted::Text(t) = extract("big.txt", huge.as_bytes()).unwrap() else {
            panic!()
        };
        assert!(t.chars().count() < MAX_EXTRACT_CHARS + 100);
        assert!(t.ends_with("beyond the index ceiling]"));
    }
}
