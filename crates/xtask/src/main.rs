//! Workspace tooling — never published (`publish = false`).
//!
//! Subcommands:
//!
//! - `bench-corpus`: encode every URL in a corpus file once, reporting wall time, throughput, latency percentiles, and aggregate compression ratio.
//! - `bench-tranco`: stream-generate synthetic URLs across real Tranco domains and benchmark encoding performance.

//!
//! Usage:
//!
//! ```sh
//! cargo run --release -p xtask -- bench-corpus crates/urlz/assets/corpus.txt
//! cargo run --release -p xtask -- bench-tranco tranco_<ID>.csv [COUNT]
//! ```

use std::{env, fs, process, time::Instant};

use urlz::encode;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: {} <subcommand> [args]\n\
 subcommands:\n \
 bench-corpus [corpus.txt]\n \
 bench-tranco [tranco.csv] [COUNT]",
            args[0]
        );
        process::exit(2);
    }
    match args[1].as_str() {
        "bench-corpus" => bench_corpus(
            args.get(2)
                .map(|s| s.as_str())
                .unwrap_or("crates/urlz/assets/corpus.txt"),
        ),
        "bench-tranco" => {
            let csv_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("tranco_L5QY4.csv");
            let count = args
                .get(3)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1_000_000);
            bench_tranco(csv_path, count);
        }
        other => {
            eprintln!("unknown subcommand: {other}");
            process::exit(2);
        }
    }
}

// --- archetype/vocabulary pools for synthetic benchmarks ---

const B64URL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

const SUBREDDITS: [&str; 24] = [
    "rust",
    "programming",
    "technology",
    "science",
    "gaming",
    "movies",
    "music",
    "askreddit",
    "worldnews",
    "funny",
    "aww",
    "space",
    "history",
    "cooking",
    "woodworking",
    "photography",
    "cycling",
    "coffee",
    "books",
    "math",
    "machinelearning",
    "webdev",
    "linux",
    "mechanicalkeyboards",
];

const TITLE_WORDS: [&str; 95] = [
    "the",
    "quick",
    "brown",
    "fox",
    "jumps",
    "over",
    "lazy",
    "dog",
    "how",
    "why",
    "best",
    "guide",
    "tutorial",
    "review",
    "vs",
    "top",
    "free",
    "new",
    "old",
    "real",
    "future",
    "secret",
    "complete",
    "beginner",
    "advanced",
    "master",
    "crash",
    "deep",
    "dive",
    "into",
    "modern",
    "classic",
    "ultimate",
    "essential",
    "hidden",
    "features",
    "tips",
    "tricks",
    "hacks",
    "mistakes",
    "lessons",
    "learned",
    "building",
    "breaking",
    "fixing",
    "understanding",
    "explaining",
    "teaching",
    "learning",
    "reading",
    "writing",
    "running",
    "walking",
    "coding",
    "debugging",
    "shipping",
    "designing",
    "testing",
    "deploying",
    "scaling",
    "optimizing",
    "compress",
    "compressed",
    "encoding",
    "decoded",
    "huffman",
    "binary",
    "qr",
    "code",
    "video",
    "game",
    "book",
    "story",
    "news",
    "world",
    "city",
    "house",
    "water",
    "fire",
    "earth",
    "air",
    "light",
    "dark",
    "fast",
    "slow",
    "big",
    "small",
    "open",
    "closed",
    "first",
    "last",
    "one",
    "two",
    "three",
    "ten",
];

const QUERY_KEYS: [&str; 28] = [
    "q",
    "query",
    "search",
    "page",
    "p",
    "per_page",
    "limit",
    "offset",
    "sort",
    "order",
    "filter",
    "category",
    "tag",
    "lang",
    "locale",
    "theme",
    "view",
    "tab",
    "ref",
    "source",
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_content",
    "utm_term",
    "gclid",
    "fbclid",
    "v",
];

const BRANDS: [&str; 20] = [
    "apple",
    "google",
    "amazon",
    "netflix",
    "spotify",
    "notion",
    "figma",
    "linear",
    "stripe",
    "vercel",
    "cloudflare",
    "docker",
    "kubernetes",
    "redis",
    "postgres",
    "nginx",
    "tailscale",
    "obsidian",
    "raycast",
    "arc",
];

const CATEGORIES: [&str; 16] = [
    "electronics",
    "furniture",
    "clothing",
    "books-media",
    "sports-outdoors",
    "toys-games",
    "automotive",
    "health-beauty",
    "grocery",
    "pets",
    "office",
    "industrial",
    "garden",
    "art-crafts",
    "music-instruments",
    "computers",
];

const CITIES: [&str; 12] = [
    "new-york",
    "san-francisco",
    "london",
    "berlin",
    "paris",
    "tokyo",
    "sydney",
    "toronto",
    "amsterdam",
    "seattle",
    "austin",
    "singapore",
];

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn pick<'a>(pool: &[&'a str], r: &mut u64) -> &'a str {
    pool[(xorshift(r) % pool.len() as u64) as usize]
}

/// kebab-case slug of `words` title-words.
fn slug(r: &mut u64, words: usize) -> String {
    (0..words)
        .map(|_| pick(&TITLE_WORDS, r))
        .collect::<Vec<_>>()
        .join("-")
}

/// base64url-shaped identifier of length `len`.
fn b64id(r: &mut u64, len: usize) -> String {
    (0..len)
        .map(|_| B64URL[(xorshift(r) % B64URL.len() as u64) as usize] as char)
        .collect()
}

const EXTENSIONS: [&str; 12] = [
    "html", "json", "pdf", "png", "jpg", "webp", "svg", "xml", "csv", "txt", "mp4", "zip",
];

/// Hex-encoded identifier of length `len` (lowercase or uppercase).
fn hex_id(r: &mut u64, len: usize, upper: bool) -> String {
    const HEX_LOWER: &[u8] = b"0123456789abcdef";
    const HEX_UPPER: &[u8] = b"0123456789ABCDEF";
    let alphabet = if upper { HEX_UPPER } else { HEX_LOWER };
    (0..len)
        .map(|_| alphabet[(xorshift(r) % alphabet.len() as u64) as usize] as char)
        .collect()
}

/// UUID-v4 shaped identifier: 8-4-4-4-12 hex.
fn uuid_v4(r: &mut u64) -> String {
    format!(
        "{}-{}-{}-{}-{}",
        hex_id(r, 8, false),
        hex_id(r, 4, false),
        hex_id(r, 4, false),
        hex_id(r, 4, false),
        hex_id(r, 12, false),
    )
}

fn bench_corpus(corpus_path: &str) {
    let data = fs::read_to_string(corpus_path).unwrap_or_else(|e| {
        eprintln!("failed to read {corpus_path}: {e}");
        process::exit(1);
    });

    let (mut src_chars, mut enc_chars) = (0u64, 0u64);
    let (mut q_src, mut q_enc, mut q_ok) = (0u64, 0u64, 0u64);
    let (mut n_src, mut n_enc, mut n_ok) = (0u64, 0u64, 0u64);
    let mut ok = 0u64;
    let mut errs = 0u64;
    let mut us: Vec<u32> = Vec::with_capacity(data.lines().count());

    let wall = Instant::now();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let start = Instant::now();
        let result = encode(std::hint::black_box(line));
        us.push(start.elapsed().as_micros() as u32);
        match result {
            Ok(payload) => {
                let sc = line.chars().count() as u64;
                let ec = payload.chars().count() as u64;
                src_chars += sc;
                enc_chars += ec;
                ok += 1;
                if line.contains('?') {
                    q_src += sc;
                    q_enc += ec;
                    q_ok += 1;
                } else {
                    n_src += sc;
                    n_enc += ec;
                    n_ok += 1;
                }
            }
            Err(_) => errs += 1,
        }
    }
    let total = wall.elapsed();

    if us.is_empty() || ok == 0 {
        eprintln!("no URLs successfully processed in corpus");
        return;
    }

    us.sort_unstable();
    let last = us.len() - 1;
    let pct = |p: f64| us[(last as f64 * p).round() as usize];

    println!("urls encoded : {ok}");
    println!("encode errors : {errs}");
    println!("wall time : {:.2}s", total.as_secs_f64());
    println!("throughput : {:.0} urls/s", ok as f64 / total.as_secs_f64());
    println!(
        "mean latency : {:.2} µs/url",
        total.as_secs_f64() * 1e6 / ok as f64
    );
    println!(
        "p50 / p90 / p99: {} / {} / {} µs",
        pct(0.50),
        pct(0.90),
        pct(0.99)
    );
    println!("max : {} µs", us[last]);
    println!("source chars : {src_chars}");
    println!("payload chars : {enc_chars}");
    println!(
        "ratio (src/enc): {:.3}",
        src_chars as f64 / enc_chars as f64
    );
    println!("--- split ---");
    println!(
        "with query : {q_ok} urls | ratio {:.3}",
        q_src as f64 / q_enc as f64
    );
    println!(
        "without query: {n_ok} urls | ratio {:.3}",
        n_src as f64 / n_enc as f64
    );
}

fn bench_tranco(csv_path: &str, count: usize) {
    use std::io::BufRead;
    let file = fs::File::open(csv_path).unwrap_or_else(|e| {
        eprintln!("failed to read {csv_path}: {e}");
        process::exit(1);
    });
    let reader = std::io::BufReader::with_capacity(8 * 1024 * 1024, file);

    let mut domains: Vec<String> = Vec::with_capacity(count.min(10_000_000));
    let mut seen = std::collections::HashSet::new();
    let mut header_skipped = false;
    for line_res in reader.lines() {
        let Ok(line) = line_res else { break };
        let Some((rank, rest)) = line.split_once(',') else {
            continue;
        };
        if !header_skipped && rank.parse::<u64>().is_err() {
            header_skipped = true;
            continue;
        }
        let domain = match rest.split_once(',') {
            Some((d, _)) => d,
            None => rest,
        };
        let label = domain.rsplit_once('.').map_or(domain, |(l, _)| l);
        if label.contains('.') || label.len() < 3 || domains.len() >= count {
            continue;
        }
        if seen.insert(domain.to_string()) {
            domains.push(domain.to_string());
        }
    }
    eprintln!("loaded {} unique domains from {csv_path}", domains.len());

    if domains.is_empty() || count == 0 {
        eprintln!("no domains loaded or count is 0");
        return;
    }

    let (mut src_chars, mut enc_chars) = (0u64, 0u64);
    let (mut q_src, mut q_enc, mut q_ok) = (0u64, 0u64, 0u64);
    let (mut n_src, mut n_enc, mut n_ok) = (0u64, 0u64, 0u64);
    let mut ok = 0u64;
    let mut errs = 0u64;
    let mut us: Vec<u32> = Vec::with_capacity(count);

    let mut r: u64 = 0x9E37_79B9_7F4A_7C15;
    let wall = Instant::now();

    for i in 0..count {
        if i > 0 && i % 1_000_000 == 0 {
            eprintln!(
                "progress: {:>8} / {} URLs ({:>5.1}%) | elapsed: {:>5.1}s | throughput: {:>6.0} urls/s",
                i,
                count,
                (i as f64 / count as f64) * 100.0,
                wall.elapsed().as_secs_f64(),
                i as f64 / wall.elapsed().as_secs_f64()
            );
        }
        let domain = &domains[i % domains.len()];
        let n = i + 1;
        r = r
            .wrapping_add(0x517C_C1B7_2722_0A95u64)
            .rotate_left((n % 61) as u32);

        let url = match i % 14 {
            0 => format!(
                "https://www.{}/r/{}/comments/{}/{}",
                domain,
                pick(&SUBREDDITS, &mut r),
                b64id(&mut r, 7),
                slug(&mut r, 3 + (i % 4))
            ),
            1 => format!(
                "https://www.{}/watch?v={}&t={}&list=PL{}",
                domain,
                b64id(&mut r, 11),
                xorshift(&mut r) % 3600,
                b64id(&mut r, 16)
            ),
            2 => format!(
                "https://{}.example.com/{}/{}/{}.{}?utm_source={}&utm_medium=cpc",
                pick(&BRANDS, &mut r),
                pick(&CATEGORIES, &mut r),
                b64id(&mut r, 8),
                slug(&mut r, 3),
                pick(&EXTENSIONS, &mut r),
                pick(&SUBREDDITS, &mut r)
            ),
            3 => format!(
                "https://{}/api/v{}/users/{}/posts?limit={}&offset={}&sort={}",
                domain,
                1 + i % 3,
                uuid_v4(&mut r),
                10 + i % 90,
                (xorshift(&mut r) % 5000) as usize,
                if i % 2 == 0 { "newest" } else { "top" }
            ),
            4 => format!(
                "https://{}/{}/{}/pull/{}/files",
                domain,
                pick(&BRANDS, &mut r),
                slug(&mut r, 2),
                100 + xorshift(&mut r) % 9000
            ),
            5 => format!(
                "https://{}/{}/{}/commit/{}",
                domain,
                pick(&BRANDS, &mut r),
                slug(&mut r, 2),
                hex_id(&mut r, 40, false)
            ),
            6 => format!(
                "https://{}/docs/v{}/{}.{}#section-{}",
                domain,
                1 + i % 5,
                slug(&mut r, 3),
                pick(&EXTENSIONS, &mut r),
                i % 12
            ),
            7 => format!(
                "https://{}/%E6%97%A5%E6%9C%AC%E8%AA%9E/{}/{}%20?q=%E3%81%8B%E3%81%88%E3%81%A7&page={}",
                domain,
                b64id(&mut r, 6),
                slug(&mut r, 2),
                1 + i % 30
            ),
            8 => format!(
                "https://{}.example.com/{}/{}/{}?color={}&size={}",
                pick(&BRANDS, &mut r),
                pick(&CATEGORIES, &mut r),
                b64id(&mut r, 9),
                slug(&mut r, 3),
                pick(&CITIES, &mut r),
                ["xs", "s", "m", "l", "xl"][i % 5]
            ),
            9 => format!(
                "https://{}/static/assets/images/{}.{}",
                domain,
                hex_id(&mut r, 16, false),
                pick(&EXTENSIONS, &mut r)
            ),
            10 => format!(
                "https://{}/item/{}/~{}/details",
                domain,
                xorshift(&mut r) % 1_000_000,
                slug(&mut r, 2)
            ),
            11 => format!(
                "https://{}/download/{}/{}.zip?token={}&expires={}",
                domain,
                uuid_v4(&mut r),
                slug(&mut r, 2),
                hex_id(&mut r, 32, true),
                1700000000 + (xorshift(&mut r) % 50000000)
            ),
            12 => format!(
                "https://{}/search?q={}&{}={}&filter=all&page={}",
                domain,
                slug(&mut r, 2 + i % 3),
                pick(&QUERY_KEYS, &mut r),
                b64id(&mut r, 6),
                1 + (i % 20)
            ),
            _ => format!(
                "https://{}/blog/{}/{}.html?ref={}",
                domain,
                2024 - (i % 5),
                slug(&mut r, 3 + (i % 3)),
                pick(&QUERY_KEYS, &mut r)
            ),
        };

        let start = Instant::now();
        let result = encode(std::hint::black_box(&url));
        us.push(start.elapsed().as_micros() as u32);
        match result {
            Ok(payload) => {
                let sc = url.chars().count() as u64;
                let ec = payload.chars().count() as u64;
                src_chars += sc;
                enc_chars += ec;
                ok += 1;
                if url.contains('?') {
                    q_src += sc;
                    q_enc += ec;
                    q_ok += 1;
                } else {
                    n_src += sc;
                    n_enc += ec;
                    n_ok += 1;
                }
            }
            Err(_) => errs += 1,
        }
    }
    let total = wall.elapsed();

    if us.is_empty() || ok == 0 {
        eprintln!("no URLs successfully processed");
        return;
    }

    us.sort_unstable();
    let last = us.len() - 1;
    let pct = |p: f64| us[(last as f64 * p).round() as usize];

    println!("=======================================================");
    println!("Tranco 1 Million Benchmark Results (1,000,000 URLs)");
    println!("=======================================================");
    println!("urls encoded : {ok}");
    println!("encode errors : {errs}");
    println!("wall time : {:.2}s", total.as_secs_f64());
    println!("throughput : {:.0} urls/s", ok as f64 / total.as_secs_f64());
    println!(
        "mean latency : {:.2} µs/url",
        total.as_secs_f64() * 1e6 / ok as f64
    );
    println!(
        "p50 / p90 / p99: {} / {} / {} µs",
        pct(0.50),
        pct(0.90),
        pct(0.99)
    );
    println!("max : {} µs", us[last]);
    println!("source chars : {src_chars}");
    println!("payload chars : {enc_chars}");
    println!(
        "ratio (src/enc): {:.3} (total compression: {:.1}%)",
        src_chars as f64 / enc_chars as f64,
        (1.0 - (enc_chars as f64 / src_chars as f64)) * 100.0
    );
    println!("--- split ---");
    println!(
        "with query : {q_ok} urls | ratio {:.3} ({:.1}% smaller)",
        q_src as f64 / q_enc as f64,
        (1.0 - (q_enc as f64 / q_src as f64)) * 100.0
    );
    println!(
        "without query: {n_ok} urls | ratio {:.3} ({:.1}% smaller)",
        n_src as f64 / n_enc as f64,
        (1.0 - (n_enc as f64 / n_src as f64)) * 100.0
    );
    println!("=======================================================");
}
