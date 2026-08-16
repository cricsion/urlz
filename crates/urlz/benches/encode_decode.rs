use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

/// Representative URLs covering the compression dictionary's interesting cases.
const URLS: &[(&str, &str)] = &[
    ("dict_hit", "https://github.com/rust-lang/rust"),
    ("dict_escape", "https://example-site.com/x"),
    ("deep_path", "https://example.com/a/b/c/d/e"),
    (
        "long_query",
        "https://example.com/search?q=rust+url+compression&page=2&sort=desc&filter=all",
    ),
    (
        "unicode_pct",
        "https://example.com/%E4%B8%AD%E6%96%87/%E8%B7%AF%E5%BE%84",
    ),
    ("fragment", "https://example.com/page#section-2"),
    ("non_default_port", "https://example.com:8080/path"),
    ("index_html", "https://example.com/index.html"),
    ("bare_host", "https://example.com"),
    ("www_google", "https://www.google.com/search?q=hello+world"),
];

fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode");
    for (name, url) in URLS {
        group.bench_with_input(BenchmarkId::new("encode", name), url, |b, u| {
            b.iter(|| black_box(urlz::encode::encode(u)))
        });
    }
    group.finish();
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");
    for (name, url) in URLS {
        let payload = urlz::encode::encode(url).expect("encode should succeed");
        group.bench_with_input(BenchmarkId::new("decode", name), &payload, |b, p| {
            b.iter(|| black_box(urlz::decode::decode(p)))
        });
    }
    group.finish();
}

fn bench_bytes_per_char(c: &mut Criterion) {
    let mut group = c.benchmark_group("bytes_per_char");
    println!("bytes-per-char summary (src_chars / enc_chars):");
    for (name, url) in URLS {
        let src_len = url.chars().count();
        let enc = urlz::encode::encode(url).expect("encode should succeed");
        let enc_len = enc.chars().count();
        println!(
            " {name}: src={src_len} enc={enc_len} ratio={:.3}",
            src_len as f64 / enc_len as f64
        );
        group.bench_with_input(BenchmarkId::new("ratio", name), url, |b, u| {
            b.iter(|| {
                let s = u.chars().count();
                let e = urlz::encode::encode(u)
                    .expect("encode should succeed")
                    .chars()
                    .count();
                black_box(s as f64 / e as f64)
            })
        });
    }
    group.finish();
}

fn bench_decode_adversarial(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_adversarial");

    // (a) Large base85 payload (~65536 decoded bytes). A ~50KB repeated-path URL
    // compresses to 19 chars (dictionary wins), so synthesize the payload
    // directly from 65536 bytes of 0xff via the library's base85 helpers.
    let big_url = format!("https://example.com/{}", "a".repeat(50_000));
    let big_payload = match urlz::encode::encode(&big_url) {
        Ok(p) if p.chars().count() > 10_000 => p,
        _ => urlz::alphabet::to_base(
            &urlz::alphabet::biguint_from_bytes_be(&vec![0xff; 65536]),
            urlz::alphabet::BASE85_ALPHABET,
        ),
    };

    // (b) Garbage that is not a valid payload.
    let garbage = "!".repeat(1024);

    // (c) A payload encoding segment_count = 64 (64 path segments).
    let seg_url = format!(
        "https://example.com/{}",
        (0..64)
            .map(|i| format!("seg{i}"))
            .collect::<Vec<_>>()
            .join("/")
    );
    let seg_payload = urlz::encode::encode(&seg_url).expect("encode should succeed");

    let adversarial: &[(&str, String)] = &[
        ("large_65536", big_payload),
        ("garbage_1024", garbage),
        ("segments_64", seg_payload),
    ];
    println!(
        "adversarial payloads: large={} chars, garbage={} chars, segments={} chars",
        adversarial[0].1.chars().count(),
        adversarial[1].1.chars().count(),
        adversarial[2].1.chars().count()
    );

    for (name, payload) in adversarial {
        group.bench_with_input(BenchmarkId::new("decode", name), payload, |b, p| {
            b.iter(|| black_box(urlz::decode::decode(p)))
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_encode,
    bench_decode,
    bench_bytes_per_char,
    bench_decode_adversarial
);
criterion_main!(benches);
