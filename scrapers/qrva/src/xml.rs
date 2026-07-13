use quick_xml::events::Event;
use quick_xml::Reader;
use serde_json::{Map, Value};

/// Parse a single `<QRVADOC>` XML record from the QRVA archive into the same
/// flat JSON shape the API returns for detail endpoints.
pub fn parse_qrva_xml(xml: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut fields: Map<String, Value> = Map::new();
    let mut current_tag: Option<String> = None;
    let mut text_parts: Vec<String> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = local_name(&e)?;
                if name == "br" {
                    if current_tag.is_some() {
                        text_parts.push('\n'.to_string());
                    }
                } else if !matches!(name.as_str(), "QRVADOC" | "link") {
                    current_tag = Some(name);
                    text_parts.clear();
                }
            }
            Ok(Event::Empty(e)) => {
                if local_name(&e)? == "br" && current_tag.is_some() {
                    text_parts.push('\n'.to_string());
                }
            }
            Ok(Event::Text(e)) => {
                if current_tag.is_some() {
                    let text = e.unescape()?.into_owned();
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        text_parts.push(trimmed.to_string());
                    }
                }
            }
            Ok(Event::CData(e)) => {
                if current_tag.is_some() {
                    let text = String::from_utf8_lossy(e.as_ref()).trim().to_string();
                    if !text.is_empty() {
                        text_parts.push(text);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if current_tag.as_deref() == Some(name.as_str()) {
                    fields.insert(name, Value::String(normalize_field_text(&text_parts.join(""))));
                    current_tag = None;
                    text_parts.clear();
                }
            }
            Ok(_) => {}
            Err(err) => return Err(err.into()),
        }
        buf.clear();
    }

    Ok(Value::Object(fields))
}

fn local_name(event: &quick_xml::events::BytesStart) -> Result<String, Box<dyn std::error::Error>> {
    Ok(String::from_utf8_lossy(event.name().as_ref()).into_owned())
}

fn normalize_field_text(text: &str) -> String {
    let mut out = String::new();
    let mut prev_newline = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_newline && !out.is_empty() {
                out.push('\n');
                prev_newline = true;
            }
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(trimmed);
        prev_newline = false;
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::build_staging_from_records;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<QRVADOC>
   <link rel="self" href="http://data.dekamer.be/v0/qrva/56-B001-1-0001-0000202400002"/>
   <SDOCNAME>56-B001-1-0001-0000202400002</SDOCNAME>
   <ID>293062</ID>
   <STATUSQ>answerReceived</STATUSQ>
   <DOCNAME>0000202400002</DOCNAME>
   <AUT>Eva
      Demesmaeker,
      N-VA (07999)</AUT>
   <TITN>De kabinetsmedewerkers van ministers.</TITN>
   <TITF>Collaborateurs des cabinets ministériels.</TITF>
   <TEXTQN>
      <br/>Onder de regering Dehaene</TEXTQN>
   <TEXTQF>
      <br/>Sous les gouvernements Dehaene</TEXTQF>
   <DEPTNUM>1380</DEPTNUM>
   <DEPTN>Eerste Minister</DEPTN>
   <DEPTF>Premier Ministre</DEPTF>
   <QUESTNUM>1</QUESTNUM>
   <STATUSA1>publicated</STATUSA1>
   <TEXTA1N>
      <br/>1. Artikel 8</TEXTA1N>
   <TEXTA1F>
      <br/>1. Article 8</TEXTA1F>
   <NUMA1>1</NUMA1>
</QRVADOC>"#;

    #[test]
    fn parses_archive_xml_into_api_shape() {
        let value = parse_qrva_xml(SAMPLE_XML).expect("xml parse");
        assert_eq!(value["DOCNAME"], "0000202400002");
        assert_eq!(value["ID"], "293062");
        assert!(value["AUT"]
            .as_str()
            .unwrap()
            .contains("Demesmaeker"));
        assert!(value["TEXTQN"].as_str().unwrap().contains("Dehaene"));
        assert!(value["TEXTA1N"].as_str().unwrap().contains("Artikel 8"));
    }

    #[test]
    fn archive_xml_builds_staging_rows() {
        let value = parse_qrva_xml(SAMPLE_XML).expect("xml parse");
        let out = build_staging_from_records(56, &[(value, "cache/a.xml".to_string())]);
        assert_eq!(out.questions.len(), 1);
        assert_eq!(out.routes.len(), 1);
        assert_eq!(out.answers.len(), 1);
        assert_eq!(out.questions[0].question_id, "56_written_0000202400002");
    }
}
