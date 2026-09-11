//! flp-extract-fxp: extract Serum 1 presets (.fxp) from FL Studio projects.

mod flp;
mod fxp;
mod serum;

use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "flp-extract-fxp",
    version,
    about = "Extract Serum 1 presets (.fxp) from FL Studio .flp projects.\n\
             The output files satisfy Serum 2's Serum-1 import checks."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the Serum plugin instances found in FLP files.
    List { inputs: Vec<PathBuf> },
    /// Extract Serum 1 presets as Serum-2-loadable .fxp files.
    Extract {
        /// FLP files to process.
        inputs: Vec<PathBuf>,
        /// Output directory (default: <flp name>_serum_fxp next to the FLP).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Overwrite existing output files.
        #[arg(long)]
        overwrite: bool,
        /// Keep extracting even when a preset fails Serum 2 validation.
        #[arg(long)]
        keep_invalid: bool,
    },
    /// Check .fxp files against Serum 2's Serum-1 import rules.
    Validate { inputs: Vec<PathBuf> },
}

/// A Serum 1 instance discovered in an FLP.
struct Instance {
    channel: Option<u16>,
    channel_name: String,
    plugin_name: String,
    chunk: serum::Serum1Chunk,
}

/// Walk the events once, associating plugin params with channel / FX names.
fn scan_serum_instances(buf: &[u8]) -> Result<(Vec<Instance>, usize, usize), String> {
    let events = flp::parse_events(buf).map_err(|e| e.to_string())?;
    let mut channels: HashMap<u16, String> = HashMap::new();
    let mut cur_channel: Option<u16> = None;
    let mut cur_fx_name = String::new();
    let mut instances = Vec::new();
    let mut serum2_count = 0usize;
    let mut failed = 0usize;

    for ev in &events {
        match ev.id {
            flp::EV_NEW_CHANNEL => {
                if ev.data.len() >= 2 {
                    cur_channel = Some(u16::from_le_bytes([ev.data[0], ev.data[1]]));
                }
            }
            flp::EV_TEXT_CHANNEL_NAME => {
                if let Some(ch) = cur_channel {
                    channels.insert(ch, text(ev.data));
                }
            }
            flp::EV_TEXT_FX_TRACK_NAME => {
                cur_fx_name = text(ev.data);
            }
            flp::EV_PLUGIN_PARAMS => {
                let Ok(pp) = flp::parse_plugin_params(ev.data) else {
                    continue;
                };
                if serum::is_serum2(pp.name, pp.filename) {
                    serum2_count += 1;
                    continue;
                }
                if !serum::is_serum1(pp.name, pp.filename) || pp.state.is_empty() {
                    continue;
                }
                match serum::serum1_chunk_from_state(pp.state) {
                    Ok(chunk) => {
                        let channel = cur_channel;
                        instances.push(Instance {
                            channel,
                            channel_name: channel
                                .and_then(|c| channels.get(&c).cloned())
                                .unwrap_or_else(|| cur_fx_name.clone()),
                            plugin_name: text(pp.name),
                            chunk,
                        });
                    }
                    Err(e) => {
                        failed += 1;
                        let where_ = cur_channel
                            .and_then(|c| channels.get(&c).cloned())
                            .unwrap_or_else(|| cur_fx_name.clone());
                        eprintln!("warning: skipped a Serum plugin state ({where_}): {e}");
                    }
                }
            }
            _ => {}
        }
    }
    Ok((instances, serum2_count, failed))
}

fn text(bytes: &[u8]) -> String {
    // FL Studio stores text events either as UTF-8 or as UTF-16LE
    // (NUL-interleaved for ASCII characters).
    if bytes.len() >= 2
        && bytes.len().is_multiple_of(2)
        && bytes.iter().skip(1).step_by(2).all(|&x| x == 0)
        && bytes.iter().step_by(2).any(|&x| x != 0)
    {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .trim()
            .to_string();
    }
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .trim()
        .to_string()
}

fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

fn sanitize_filename(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = mapped.trim().trim_matches('.').trim().to_string();
    if trimmed.is_empty() {
        "Untitled".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

fn default_out_dir(flp: &Path) -> PathBuf {
    let stem = flp
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".into());
    let mut name = sanitize_filename(&stem);
    if name.is_empty() {
        name = "output".into();
    }
    name.push_str("_serum_fxp");
    flp.parent()
        .map(|p| p.join(&name))
        .unwrap_or_else(|| name.into())
}

fn run_extract(
    inputs: &[PathBuf],
    out: Option<&PathBuf>,
    overwrite: bool,
    keep_invalid: bool,
) -> Result<(), String> {
    let mut total_extracted = 0usize;
    let mut total_invalid = 0usize;
    for input in inputs {
        let buf = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
        let (instances, serum2_count, _failed) = scan_serum_instances(&buf)?;
        if instances.is_empty() {
            eprintln!(
                "{}: no Serum 1 presets found ({} Serum 2 instance(s) skipped)",
                input.display(),
                serum2_count
            );
            continue;
        }
        let out_dir = match out {
            Some(o) => o.clone(),
            None => default_out_dir(input),
        };
        std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
        println!(
            "{}: {} Serum 1 preset(s){}",
            input.display(),
            instances.len(),
            if serum2_count > 0 {
                format!(", {serum2_count} Serum 2 instance(s) skipped")
            } else {
                String::new()
            }
        );
        let mut seen: HashMap<u64, ()> = HashMap::new();
        let mut used_names: HashMap<String, usize> = HashMap::new();
        for (i, inst) in instances.iter().enumerate() {
            let key = hash_bytes(&inst.chunk.chunk);
            if seen.contains_key(&key) {
                println!(
                    "  [{:02}] duplicate of an earlier preset, skipped (channel '{}')",
                    i + 1,
                    inst.channel_name
                );
                continue;
            }
            seen.insert(key, ());

            let report = fxp::validate_chunk_report(&inst.chunk.chunk);
            let file = fxp::build_fxp(&inst.chunk.chunk, &inst.chunk.meta.preset_name);

            let base_name: String = if !inst.chunk.meta.preset_name.is_empty() {
                inst.chunk.meta.preset_name.clone()
            } else if !inst.channel_name.is_empty() {
                inst.channel_name.clone()
            } else {
                format!("Serum {}", i + 1)
            };
            let base = sanitize_filename(&base_name);
            let count = used_names.entry(base.clone()).or_insert(0);
            *count += 1;
            let file_name = if *count == 1 {
                format!("{:02}_{}.fxp", i + 1, base)
            } else {
                format!("{:02}_{}_{}.fxp", i + 1, base, count)
            };
            let path = out_dir.join(&file_name);
            if path.exists() && !overwrite {
                eprintln!(
                    "  [{:02}] {} exists, skipped (use --overwrite)",
                    i + 1,
                    path.display()
                );
                continue;
            }
            if !report.is_ok() && !keep_invalid {
                total_invalid += 1;
                for f in report.fatals() {
                    eprintln!("  [{:02}] INVALID preset '{}': {}", i + 1, base, f);
                }
                continue;
            }
            std::fs::write(&path, &file).map_err(|e| format!("{}: {e}", path.display()))?;
            total_extracted += 1;
            let wt = if inst.chunk.stream_sizes.len() > 1 {
                format!(
                    ", {} embedded table(s): {}",
                    inst.chunk.stream_sizes.len() - 1,
                    inst.chunk.stream_sizes[1..]
                        .iter()
                        .map(|s| format_bytes(*s))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                String::new()
            };
            println!(
                "  [{:02}] channel '{}' -> preset '{}' by '{}' (cat '{}', state {}, ver {:.4}{wt}) => {}",
                i + 1,
                if inst.channel_name.is_empty() {
                    "-"
                } else {
                    &inst.channel_name
                },
                if inst.chunk.meta.preset_name.is_empty() {
                    "-"
                } else {
                    &inst.chunk.meta.preset_name
                },
                if inst.chunk.meta.author.is_empty() {
                    "-"
                } else {
                    &inst.chunk.meta.author
                },
                if inst.chunk.meta.category.is_empty() {
                    "-"
                } else {
                    &inst.chunk.meta.category
                },
                format_bytes(inst.chunk.stream_sizes.first().copied().unwrap_or(0)),
                inst.chunk.meta.version_f32,
                path.display(),
            );
            for w in report.warnings() {
                println!("       note: {w}");
            }
        }
    }
    if total_invalid > 0 {
        return Err(format!(
            "{total_invalid} preset(s) failed Serum 2 validation (see above)"
        ));
    }
    if total_extracted == 0 {
        return Err("no presets extracted".into());
    }
    Ok(())
}

fn run_list(inputs: &[PathBuf]) -> Result<(), String> {
    let mut any = false;
    for input in inputs {
        let buf = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
        let (instances, serum2_count, _failed) = scan_serum_instances(&buf)?;
        println!(
            "{}: {} Serum 1 preset(s), {} Serum 2 instance(s)",
            input.display(),
            instances.len(),
            serum2_count
        );
        any = any || !instances.is_empty();
        for (i, inst) in instances.iter().enumerate() {
            println!(
                "  [{:02}] channel {} '{}' plugin '{}' -> preset '{}' (author '{}', state {} bytes, {} stream(s), source {:?})",
                i + 1,
                inst.channel.map(|c| c.to_string()).unwrap_or("-".into()),
                inst.channel_name,
                inst.plugin_name,
                inst.chunk.meta.preset_name,
                inst.chunk.meta.author,
                inst.chunk.stream_sizes.first().copied().unwrap_or(0),
                inst.chunk.stream_sizes.len(),
                inst.chunk.source,
            );
        }
    }
    if !any {
        eprintln!("no Serum 1 presets found in any input");
    }
    Ok(())
}

fn run_validate(inputs: &[PathBuf]) -> Result<(), String> {
    let mut all_ok = true;
    for input in inputs {
        let data = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
        let ext_ok = input
            .extension()
            .map(|e| e.eq_ignore_ascii_case("fxp"))
            .unwrap_or(false);
        let mut report = fxp::validate_fxp(&data);
        if !ext_ok {
            report.issues.push(fxp::ValidationIssue {
                severity: fxp::Severity::Fatal,
                message:
                    "file extension is not .fxp (Serum 2 only offers the import for .fxp names)"
                        .to_string(),
            });
        }
        let status = if report.is_ok() { "PASS" } else { "FAIL" };
        println!("{}: {}", input.display(), status);
        for f in report.fatals() {
            all_ok = false;
            println!("  error: {f}");
        }
        for w in report.warnings() {
            println!("  note: {w}");
        }
        if report.is_ok() {
            // informational: decompress and show the embedded preset name
            if let Ok((state, _)) = fxp::inflate_state(&data)
                && state.len() > serum::OFF_PRESET_NAME
            {
                let name = String::from_utf8_lossy(
                    &state[serum::OFF_PRESET_NAME..serum::OFF_PRESET_NAME + 32],
                );
                let name = name.trim_end_matches('\0').trim();
                println!("  preset: {name}");
            }
        }
    }
    if !all_ok {
        return Err("validation failed".into());
    }
    Ok(())
}

fn format_bytes(n: usize) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::List { inputs } => run_list(inputs),
        Command::Extract {
            inputs,
            out,
            overwrite,
            keep_invalid,
        } => run_extract(inputs, out.as_ref(), *overwrite, *keep_invalid),
        Command::Validate { inputs } => run_validate(inputs),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
