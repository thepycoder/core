mod io;
mod parse;
mod xml;

use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::qrva_text::QRVA_API_BASE;
use crawl::utils::relative_cache_path;
use io::{write_written_answers, write_written_questions, write_written_routes};
use parse::{build_staging_from_records, records_from_search_page};
use xml::parse_qrva_xml;
use serde_json::Value;
use std::error::Error;
use std::fs;

const SESSION_ID: u32 = 56;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = ScrapingClient::new();
    let cache_root = cache_dir();
    let data_root = data_dir();

    let detail_dir = cache_root.join(format!("sessions/{SESSION_ID}/qrva/detail"));
    fs::create_dir_all(&detail_dir)?;

    let archive_path = cache_root.join(format!("sessions/{SESSION_ID}/qrva/archive/QRVA_{SESSION_ID}.zip"));
    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let archive_url = format!("{QRVA_API_BASE}/qrva/archive?leg={SESSION_ID}");
    if cache_only() {
        if !archive_path.exists() {
            eprintln!(
                "QRVA archive cache missing at {} — loading detail/*.json only",
                archive_path.display()
            );
        }
    } else {
        let archive_resp = client.get(&archive_url).await?;
        if archive_resp.status().is_success() {
            let bytes = archive_resp.bytes().await?;
            fs::write(&archive_path, &bytes)?;
            eprintln!("Cached QRVA archive ({} bytes)", bytes.len());
        } else {
            eprintln!(
                "QRVA archive unavailable ({}), falling back to paginated search",
                archive_resp.status()
            );
        }
    }

    let mut records: Vec<(Value, String)> = Vec::new();

    if archive_path.exists() {
        let file = fs::File::open(&archive_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut entry, &mut buf)?;
            let items = if name.ends_with(".xml") {
                vec![parse_qrva_xml(&buf)?]
            } else if name.ends_with(".json") {
                let body: Value = serde_json::from_str(&buf)?;
                if body.get("DOCNAME").is_some() || body.get("docname").is_some() {
                    vec![body]
                } else {
                    records_from_search_page(&body)
                }
            } else {
                continue;
            };
            let cache_path = relative_cache_path(
                &detail_dir.join(sanitize_filename(&name)),
                &cache_root,
            );
            for item in items {
                records.push((item, cache_path.clone()));
            }
        }
    }

    if records.is_empty() && cache_only() {
        for entry in fs::read_dir(&detail_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let body: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
            let cache_path = relative_cache_path(&path, &cache_root);
            let detail_items = records_from_search_page(&body);
            if detail_items.is_empty() {
                records.push((body, cache_path));
            } else {
                for item in detail_items {
                    records.push((item, cache_path.clone()));
                }
            }
        }
    }

    if records.is_empty() && !cache_only() {
        let mut start = 0;
        loop {
            let url = format!("{QRVA_API_BASE}/qrva?leg={SESSION_ID}&start={start}");
            let resp = client.get_json(&url).await?;
            if !resp.status().is_success() {
                break;
            }
            let body: Value = serde_json::from_str(&resp.text().await?)?;
            let total = body.get("total").and_then(|v| v.as_i64()).unwrap_or(0);
            let page_items = records_from_search_page(&body);
            if page_items.is_empty() {
                break;
            }
            for item in page_items {
                let sdocname = item
                    .get("SDOCNAME")
                    .or_else(|| item.get("sdocname"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let detail_url = format!("{QRVA_API_BASE}/qrva/{sdocname}");
                let detail_resp = client.get_json(&detail_url).await?;
                if !detail_resp.status().is_success() {
                    continue;
                }
                let detail_body: Value = serde_json::from_str(&detail_resp.text().await?)?;
                let cache_file = detail_dir.join(format!("{sdocname}.json"));
                fs::write(&cache_file, serde_json::to_string_pretty(&detail_body)?)?;
                let cache_path =
                    relative_cache_path(&cache_file, &cache_root);
                let detail_items = records_from_search_page(&detail_body);
                if detail_items.is_empty() {
                    records.push((detail_body, cache_path));
                } else {
                    for d in detail_items {
                        records.push((d, cache_path.clone()));
                    }
                }
            }
            start += 10;
            if start >= total {
                break;
            }
        }
    }

    let staging = build_staging_from_records(SESSION_ID, &records);
    let written_dir = data_root.join(format!("sessions/{SESSION_ID}/written"));
    fs::create_dir_all(&written_dir)?;

    write_written_questions(&written_dir.join("questions.parquet"), &staging.questions)?;
    write_written_routes(&written_dir.join("routes.parquet"), &staging.routes)?;
    write_written_answers(&written_dir.join("answers.parquet"), &staging.answers)?;

    eprintln!(
        "QRVA staging: {} questions, {} routes, {} answers",
        staging.questions.len(),
        staging.routes.len(),
        staging.answers.len()
    );
    Ok(())
}

fn sanitize_filename(name: &str) -> String {
    name.replace('/', "_").replace('\\', "_")
}
