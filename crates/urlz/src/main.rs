use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use urlz::decode;
use urlz::encode;
use urlz::huffman;

/// urlz URL compression: encode URLs into compact payloads and build Huffman
/// codebooks from URL corpora.
///
/// note: for very short URLs the encoded payload may be longer than the source —
/// not guaranteed shorter.
#[derive(Parser)]
#[command(name = "urlz", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Encode a URL into a base85 payload
    Encode {
        /// The URL to encode
        url: String,
    },
    /// Decode a payload back into the original URL
    Decode {
        /// The payload to decode (base85)
        payload: String,
    },
    /// Build a Huffman codebook from a URL corpus
    Dict {
        #[command(subcommand)]
        command: DictCommand,
    },
}

#[derive(Subcommand)]
enum DictCommand {
    /// Build a codebook from a corpus file (omit to emit the bundled default codebook)
    Build {
        /// Corpus file with one URL per line; defaults to the bundled example corpus
        corpus: Option<PathBuf>,
        /// Output directory for the codebook
        #[arg(long, value_name = "DIR", required = true)]
        out: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), anyhow::Error> {
    match cli.command {
        Command::Encode { url } => {
            let payload = encode::encode(&url)?;
            println!("{payload}");
        }
        Command::Decode { payload } => {
            let url = decode::decode(&payload)?;
            println!("{url}");
        }
        Command::Dict { command } => match command {
            DictCommand::Build { corpus, out } => {
                let cb = match corpus {
                    Some(path) => {
                        let text = std::fs::read_to_string(&path).with_context(|| {
                            format!("failed to read corpus file {}", path.display())
                        })?;
                        let parsed = huffman::parse_corpus(&text);
                        huffman::build_from_corpus(&parsed)
                    }
                    None => huffman::default_codebook(),
                };
                std::fs::create_dir_all(&out).with_context(|| {
                    format!("failed to create output directory {}", out.display())
                })?;
                let codebook_path = out.join("codebook.bin");
                huffman::write_codebook_file(&codebook_path, &cb)?;
                let size = std::fs::metadata(&codebook_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                println!(
                    "wrote codebook to {} ({} bytes)",
                    codebook_path.display(),
                    size
                );
            }
        },
    }
    Ok(())
}
