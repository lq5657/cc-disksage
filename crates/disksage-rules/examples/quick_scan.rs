use std::collections::HashMap;

use disksage_rules::{load_builtin_rules, quick_scan};

fn main() {
    let env: HashMap<String, String> = std::env::vars().collect();
    let pack = load_builtin_rules().expect("built-in rules must parse");
    let include_admin = std::env::args().any(|argument| argument == "--admin");
    let started = std::time::Instant::now();
    let result = quick_scan(&pack, &env, include_admin).expect("quick scan failed");

    println!(
        "quick scan: {} findings in {:.2}s",
        result.findings.len(),
        started.elapsed().as_secs_f64()
    );
    for finding in &result.findings {
        println!(
            "\n[{}] {} ({}) risk={:?} files={} bytes={} admin={} default={}",
            finding.rule_id,
            finding.title_zh,
            finding.id,
            finding.risk,
            finding.file_count,
            finding.allocated_bytes,
            finding.requires_admin,
            finding.default_selected
        );
        println!("  consequence: {}", finding.consequence);
        for evidence in &finding.evidence {
            println!(
                "  {} [{:?}] {}",
                evidence.id, evidence.source, evidence.claim
            );
        }
        if let Some(rule) = pack.rules.iter().find(|rule| rule.id == finding.rule_id) {
            println!("  min_age_days={}", rule.min_age_days);
        }
        if let Some(files) = result.matches.get(&finding.id) {
            let newest = files
                .iter()
                .filter_map(|path| {
                    std::fs::metadata(path)
                        .ok()
                        .and_then(|metadata| metadata.modified().ok())
                })
                .max();
            println!("  newest matched mtime={newest:?}");
            println!("  matched sample:");
            for file in files.iter().take(3) {
                println!("    {}", file.display());
            }
        }
    }
}
