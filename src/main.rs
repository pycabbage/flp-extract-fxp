//! `flp-extract-fxp` command-line interface.
//!
//! The reusable logic lives in the library crate ([`flp_extract_fxp`] CRATE):
//! FLP parsing (`flp`), Serum detection (`serum`), `.fxp` building and
//! validation (`fxp`) and the shared scanning helpers (`core`). This binary
//! only wires the clap CLI around them: argument parsing, filesystem IO and
//! console output.

use clap::{Parser, Subcommand};
use flp_extract_fxp::core::{
    default_out_dir, format_bytes, hash_bytes, sanitize_filename, scan_serum_instances,
    scan_serum2_instances,
};
use flp_extract_fxp::flpconv::BundleSource;
use flp_extract_fxp::{flpconv, fxp, serum, serum2state};
use std::collections::HashMap;
use std::path::PathBuf;

/// Fallback label for report messages (`-` when empty).
fn display_name(name: &str) -> &str {
    if name.is_empty() { "-" } else { name }
}

#[derive(Parser)]
#[command(
    name = "flp-extract-fxp",
    version,
    about = "Extract Serum presets (.fxp) from FL Studio .flp projects.\n\
             The output files satisfy Serum2's Serum import checks."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the Serum plugin instances found in FLP files.
    List { inputs: Vec<PathBuf> },
    /// Extract Serum presets as Serum2-loadable .fxp files.
    Extract {
        /// FLP files to process.
        inputs: Vec<PathBuf>,
        /// Output directory (default: <flp name>_serum_fxp next to the FLP).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Overwrite existing output files.
        #[arg(long)]
        overwrite: bool,
        /// Keep extracting even when a preset fails Serum2 validation.
        #[arg(long)]
        keep_invalid: bool,
        /// Also extract Serum2 instances as .SerumPreset preset files.
        #[arg(long)]
        serum2: bool,
    },
    /// Check .fxp files against Serum2's Serum import rules.
    Validate { inputs: Vec<PathBuf> },
    /// Convert Serum instances in FLP files into Serum2 instances.
    Convert {
        /// FLP files to process.
        inputs: Vec<PathBuf>,
        /// Output FLP path (single input only; default: <input>_serum2.flp).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Print the conversion plan without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
}

fn run_extract(
    inputs: &[PathBuf],
    out: Option<&PathBuf>,
    overwrite: bool,
    keep_invalid: bool,
    serum2: bool,
) -> Result<(), String> {
    let mut total_extracted = 0usize;
    let mut total_invalid = 0usize;
    for input in inputs {
        let buf = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
        let (instances, stats) = scan_serum_instances(&buf)?;
        for msg in &stats.failed {
            eprintln!("warning: {msg}");
        }
        // Serum2 instances: only scanned (and extracted) with --serum2.
        let s2_instances = if serum2 {
            let (insts, warnings) = scan_serum2_instances(&buf)?;
            for msg in &warnings {
                eprintln!("warning: {msg}");
            }
            insts
        } else {
            Vec::new()
        };
        if instances.is_empty() && s2_instances.is_empty() {
            eprintln!(
                "{}: no Serum presets found ({} Serum2 instance(s) skipped)",
                input.display(),
                stats.serum2_count
            );
            continue;
        }
        let out_dir = match out {
            Some(o) => o.clone(),
            None => default_out_dir(input),
        };
        std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
        println!(
            "{}: {} Serum preset(s){}",
            input.display(),
            instances.len(),
            if stats.serum2_count > 0 {
                format!(", {} Serum2 instance(s) skipped", stats.serum2_count)
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

        // --serum2: write one .SerumPreset per Serum2 instance.
        if serum2 {
            let mut used_s2: HashMap<String, usize> = HashMap::new();
            for (i, inst) in s2_instances.iter().enumerate() {
                let file_bytes = match serum2state::preset_file_from_processor(
                    &inst.processor,
                    inst.meta.clone(),
                ) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        eprintln!(
                            "  [S2 {:02}] skipped a Serum2 instance (channel '{}'): {e}",
                            i + 1,
                            if inst.channel_name.is_empty() {
                                "-"
                            } else {
                                &inst.channel_name
                            }
                        );
                        continue;
                    }
                };
                let base_name = if !inst.meta.preset_name.is_empty() {
                    inst.meta.preset_name.clone()
                } else {
                    format!("Instance {}", i + 1)
                };
                let base = sanitize_filename(&base_name);
                let count = used_s2.entry(base.clone()).or_insert(0);
                *count += 1;
                let file_name = if *count == 1 {
                    format!("{:02}_{base}.SerumPreset", i + 1)
                } else {
                    format!("{:02}_{base}_{count}.SerumPreset", i + 1)
                };
                let path = out_dir.join(&file_name);
                if path.exists() && !overwrite {
                    eprintln!(
                        "  [S2 {:02}] {} exists, skipped (use --overwrite)",
                        i + 1,
                        path.display()
                    );
                    continue;
                }
                std::fs::write(&path, &file_bytes)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                total_extracted += 1;
                println!(
                    "  [S2 {:02}] channel '{}' -> preset '{}' by '{}' ({} B) => {}",
                    i + 1,
                    if inst.channel_name.is_empty() {
                        "-"
                    } else {
                        &inst.channel_name
                    },
                    display_name(&inst.meta.preset_name),
                    display_name(&inst.meta.preset_author),
                    file_bytes.len(),
                    path.display()
                );
            }
        }
    }
    if total_invalid > 0 {
        return Err(format!(
            "{total_invalid} preset(s) failed Serum2 validation (see above)"
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
        let (instances, stats) = scan_serum_instances(&buf)?;
        for msg in &stats.failed {
            eprintln!("warning: {msg}");
        }
        println!(
            "{}: {} Serum preset(s), {} Serum2 instance(s)",
            input.display(),
            instances.len(),
            stats.serum2_count
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
        eprintln!("no Serum presets found in any input");
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
                    "file extension is not .fxp (Serum2 only offers the import for .fxp names)"
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

fn run_convert(inputs: &[PathBuf], out: Option<&PathBuf>, dry_run: bool) -> Result<(), String> {
    if out.is_some() && inputs.len() > 1 {
        return Err("--out can only be used with a single input file".into());
    }
    let mut source = flpconv::RealSource::embedded();
    let mut total_converted = 0usize;
    for input in inputs {
        let buf = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
        // Single walk: the plans already carry each instance's parsed Serum
        // preset (plan.s1) and preset name; nothing re-scans the buffer.
        let (plans, scan_warnings) = flpconv::scan_convertible_detailed(&buf)?;
        if plans.is_empty() {
            for w in &scan_warnings {
                eprintln!("warning: {w}");
            }
            eprintln!("{}: no convertible Serum instances found", input.display());
            continue;
        }
        println!(
            "{}: converting {} Serum instance(s)",
            input.display(),
            plans.len()
        );
        let mut bundles: Vec<Option<flpconv::Serum2Bundle>> = Vec::with_capacity(plans.len());
        for (i, plan) in plans.iter().enumerate() {
            match source.bundle_for(plan, &[]) {
                Ok(Some(bundle)) => bundles.push(Some(bundle)),
                Ok(None) => {
                    let reason = source
                        .warnings
                        .pop()
                        .unwrap_or_else(|| "no Serum2 bundle produced".into());
                    return Err(format!(
                        "instance {} on channel '{}': {reason}",
                        i + 1,
                        if plan.channel_name.is_empty() {
                            "-"
                        } else {
                            &plan.channel_name
                        }
                    ));
                }
                Err(e) => {
                    return Err(format!(
                        "instance {} on channel '{}': {e}",
                        i + 1,
                        if plan.channel_name.is_empty() {
                            "-"
                        } else {
                            &plan.channel_name
                        }
                    ));
                }
            }
        }
        let (out_buf, report) = flpconv::apply(&buf, &plans, &bundles)?;
        for (k, c) in report.converted.iter().enumerate() {
            let state_bytes = plans[k]
                .s1
                .as_ref()
                .map(|p| p.blob.len())
                .unwrap_or_default();
            println!(
                "  [{:02}] channel '{}' preset '{}' (state {}) -> converted (cid3 {} B)",
                k + 1,
                if c.channel_name.is_empty() {
                    "-"
                } else {
                    &c.channel_name
                },
                if c.preset_name.is_empty() {
                    "-"
                } else {
                    &c.preset_name
                },
                format_bytes(state_bytes),
                c.new_payload_len,
            );
        }
        total_converted += report.converted.len();
        for w in &scan_warnings {
            eprintln!("warning: {w}");
        }
        for w in &report.warnings {
            eprintln!("warning: {w}");
        }
        if dry_run {
            println!("dry run: no files written");
            continue;
        }
        let out_path = match out {
            Some(o) => o.clone(),
            None => {
                let mut name = input
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "output".into());
                name.push_str("_serum2");
                let ext = input
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default();
                input.with_file_name(format!("{name}{ext}"))
            }
        };
        std::fs::write(&out_path, &out_buf).map_err(|e| format!("{}: {e}", out_path.display()))?;
        println!(
            "converted {} instance(s) -> {} ({} bytes)",
            report.converted.len(),
            out_path.display(),
            out_buf.len()
        );
    }
    if total_converted == 0 {
        return Err("no instances converted".into());
    }
    Ok(())
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
            serum2,
        } => run_extract(inputs, out.as_ref(), *overwrite, *keep_invalid, *serum2),
        Command::Validate { inputs } => run_validate(inputs),
        Command::Convert {
            inputs,
            out,
            dry_run,
        } => run_convert(inputs, out.as_ref(), *dry_run),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
