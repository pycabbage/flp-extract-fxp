//! `flp-extract-fxp` command-line interface.
//!
//! The reusable logic lives in the library crate ([`flp_extract_fxp`] CRATE):
//! FLP parsing (`flp`), Serum detection (`serum`), `.fxp` building and
//! validation (`fxp`) and the shared scanning helpers (`core`). This binary
//! only wires the clap CLI around them: argument parsing, filesystem IO and
//! console output.
//!
//! Output contract: every command first collects its results into a
//! structured report (`report::ListReport`, ...) while emitting the
//! human-readable lines. Without `--json` the behavior is unchanged. With
//! `--json` stdout carries exactly one pretty-printed JSON document (the
//! report, or `{"error": "..."}` when the command aborts); all
//! human-readable progress moves to stderr, and a command that completes
//! but must fail (e.g. "validation failed") still prints its full report
//! with an embedded `"error"` field and exits non-zero.

use clap::{Parser, Subcommand};
use flp_extract_fxp::core::{
    self, default_out_dir, format_bytes, hash_bytes, sanitize_filename, scan_serum_instances,
};
use flp_extract_fxp::flpconv::BundleSource;
use flp_extract_fxp::report::{
    CommandReport, ConvertDetail, ConvertInputReport, ConvertReport, ExtractEntry,
    ExtractInputReport, ExtractReport, ExtractStatus, ListInputReport, ListReport, PatchReport,
    PresetEntry, ValidateInputReport, ValidateReport,
};
use flp_extract_fxp::{flpconv, fxp, serum};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    List {
        inputs: Vec<PathBuf>,
        /// Print a machine-readable JSON report on stdout (progress goes
        /// to stderr).
        #[arg(long)]
        json: bool,
    },
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
        /// Write presets whose content hash duplicates an earlier one anyway.
        #[arg(long)]
        keep_duplicates: bool,
        /// Print a machine-readable JSON report on stdout (progress goes
        /// to stderr).
        #[arg(long)]
        json: bool,
    },
    /// Check .fxp files against Serum2's Serum import rules.
    Validate {
        inputs: Vec<PathBuf>,
        /// Print a machine-readable JSON report on stdout (progress goes
        /// to stderr).
        #[arg(long)]
        json: bool,
    },
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
        /// Print a machine-readable JSON report on stdout (progress goes
        /// to stderr).
        #[arg(long)]
        json: bool,
    },
    /// Convert standalone Serum .fxp presets into Serum2 .SerumPreset files.
    ConvertFxp {
        /// Serum .fxp preset files to process.
        inputs: Vec<PathBuf>,
        /// Output directory (default: alongside each input).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Overwrite existing output files.
        #[arg(long)]
        overwrite: bool,
        /// Print the result as a single JSON document on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Patch preset metadata in a Serum .fxp file (or in every Serum
    /// instance of an FLP project).
    Patch {
        /// A .fxp file, or a .flp project whose Serum instances are patched.
        input: PathBuf,
        /// New preset name (goes to the header AND the state blob).
        #[arg(long)]
        name: Option<String>,
        /// New author string.
        #[arg(long)]
        author: Option<String>,
        /// New category string.
        #[arg(long)]
        category: Option<String>,
        /// Output path (default: patch the input in place).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Print what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Print the result as a single JSON document on stdout.
        #[arg(long)]
        json: bool,
    },
}

/// Human-readable progress sink: stdout normally, stderr under `--json`
/// (stdout then carries the JSON document and nothing else).
struct Out {
    json: bool,
}

impl Out {
    fn line(&self, msg: &str) {
        if self.json {
            eprintln!("{msg}");
        } else {
            println!("{msg}");
        }
    }
}

/// Read one input path and resolve it into the FLP documents to process:
/// the plain FLP itself, or every `*.flp` member of a zipped loop package
/// unpacked in memory (`core::flp_inputs`).
fn read_input_docs(input: &Path) -> Result<Vec<core::FlpInput>, String> {
    let buf = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
    core::flp_inputs(&input.display().to_string(), &buf)
}

/// Print a scan warning, tagging archive members with their member name.
fn print_warning(doc: &core::FlpInput, msg: &str) {
    if doc.from_archive {
        eprintln!("warning: {}: {msg}", doc.name);
    } else {
        eprintln!("warning: {msg}");
    }
}

fn run_extract(
    inputs: &[PathBuf],
    out_dir_opt: Option<&PathBuf>,
    overwrite: bool,
    keep_invalid: bool,
    keep_duplicates: bool,
    out: &Out,
) -> Result<ExtractReport, String> {
    let mut input_reports: Vec<ExtractInputReport> = Vec::new();
    let mut total_extracted = 0usize;
    let mut total_invalid = 0usize;
    // content hash -> first occurrence (file, 1-based preset index), shared
    // across the whole batch so duplicates are detected across input files.
    let mut seen: HashMap<u64, (String, usize)> = HashMap::new();
    for input in inputs {
        let docs = read_input_docs(input)?;
        let out_dir = match out_dir_opt {
            Some(o) => o.clone(),
            None => default_out_dir(input),
        };
        // Unique naming spans the whole input (all members of a zipped loop
        // package); dedupe spans the whole batch (see `seen` above).
        let mut used_names: HashMap<String, usize> = HashMap::new();
        for doc in &docs {
            let (instances, stats) = match scan_serum_instances(&doc.data) {
                Ok(v) => v,
                Err(e) => {
                    if doc.from_archive {
                        eprintln!("warning: skipping {}: {e}", doc.name);
                        continue;
                    }
                    return Err(e);
                }
            };
            for msg in &stats.failed {
                print_warning(doc, msg);
            }
            if instances.is_empty() {
                eprintln!(
                    "{}: no Serum presets found ({} Serum2 instance(s) skipped)",
                    doc.name, stats.serum2_count
                );
                input_reports.push(ExtractInputReport {
                    input: doc.name.clone(),
                    out_dir: out_dir.display().to_string(),
                    entries: Vec::new(),
                    extracted_count: 0,
                    failed: stats.failed,
                    serum2_skipped: stats.serum2_count as u32,
                });
                continue;
            }
            std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
            out.line(&format!(
                "{}: {} Serum preset(s){}",
                doc.name,
                instances.len(),
                if stats.serum2_count > 0 {
                    format!(", {} Serum2 instance(s) skipped", stats.serum2_count)
                } else {
                    String::new()
                }
            ));
            let mut entries: Vec<ExtractEntry> = Vec::with_capacity(instances.len());
            let mut extracted_count = 0u32;
            for (i, inst) in instances.iter().enumerate() {
                let key = hash_bytes(&inst.chunk.chunk);
                let report = fxp::validate_chunk_report(&inst.chunk.chunk);
                let warnings: Vec<String> = report.warnings().map(str::to_string).collect();
                let errors: Vec<String> = report.fatals().map(str::to_string).collect();
                let base_name: String = if !inst.chunk.meta.preset_name.is_empty() {
                    inst.chunk.meta.preset_name.clone()
                } else if !inst.channel_name.is_empty() {
                    inst.channel_name.clone()
                } else {
                    format!("Serum {}", i + 1)
                };
                let base = sanitize_filename(&base_name);
                let mut push_entry = |status: ExtractStatus, path: Option<String>| {
                    entries.push(ExtractEntry {
                        index: i as u32,
                        channel: inst.channel,
                        channel_name: inst.channel_name.clone(),
                        preset_name: inst.chunk.meta.preset_name.clone(),
                        author: inst.chunk.meta.author.clone(),
                        category: inst.chunk.meta.category.clone(),
                        version_f32: inst.chunk.meta.version_f32,
                        state_bytes: inst.chunk.stream_sizes.first().copied().unwrap_or(0),
                        chunk_bytes: inst.chunk.chunk.len(),
                        source: format!("{:?}", inst.chunk.source),
                        status,
                        path,
                        valid: report.is_ok(),
                        warnings: warnings.clone(),
                        errors: errors.clone(),
                    })
                };

                if let Some((first_file, first_idx)) = seen.get(&key) {
                    out.line(&format!(
                        "  [{:02}] duplicate of {first_file}:{first_idx:02}, skipped (channel '{}')",
                        i + 1,
                        inst.channel_name
                    ));
                    if !keep_duplicates {
                        push_entry(ExtractStatus::Duplicate, None);
                        continue;
                    }
                } else {
                    seen.insert(key, (doc.name.clone(), i + 1));
                }

                let file = fxp::build_fxp(&inst.chunk.chunk, &inst.chunk.meta.preset_name);
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
                    push_entry(ExtractStatus::Exists, None);
                    continue;
                }
                if !report.is_ok() && !keep_invalid {
                    total_invalid += 1;
                    for f in report.fatals() {
                        eprintln!("  [{:02}] INVALID preset '{}': {}", i + 1, base, f);
                    }
                    push_entry(ExtractStatus::Invalid, None);
                    continue;
                }
                std::fs::write(&path, &file).map_err(|e| format!("{}: {e}", path.display()))?;
                total_extracted += 1;
                extracted_count += 1;
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
                out.line(&format!(
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
                ));
                for w in &warnings {
                    out.line(&format!("       note: {w}"));
                }
                push_entry(ExtractStatus::Written, Some(path.display().to_string()));
            }
            input_reports.push(ExtractInputReport {
                input: doc.name.clone(),
                out_dir: out_dir.display().to_string(),
                extracted_count,
                entries,
                failed: stats.failed,
                serum2_skipped: stats.serum2_count as u32,
            });
        }
    }
    let mut report = ExtractReport {
        inputs: input_reports,
        extracted_count: total_extracted as u32,
        invalid_count: total_invalid as u32,
        error: None,
    };
    if total_invalid > 0 {
        report.error = Some(format!(
            "{total_invalid} preset(s) failed Serum2 validation (see above)"
        ));
    } else if total_extracted == 0 {
        report.error = Some("no presets extracted".into());
    }
    Ok(report)
}

fn run_list(inputs: &[PathBuf], out: &Out) -> Result<ListReport, String> {
    let mut input_reports: Vec<ListInputReport> = Vec::new();
    let mut total_presets = 0usize;
    for input in inputs {
        for doc in read_input_docs(input)? {
            let (instances, stats) = match scan_serum_instances(&doc.data) {
                Ok(v) => v,
                Err(e) => {
                    if doc.from_archive {
                        eprintln!("warning: skipping {}: {e}", doc.name);
                        continue;
                    }
                    return Err(e);
                }
            };
            for msg in &stats.failed {
                print_warning(&doc, msg);
            }
            out.line(&format!(
                "{}: {} Serum preset(s), {} Serum2 instance(s)",
                doc.name,
                instances.len(),
                stats.serum2_count
            ));
            total_presets += instances.len();
            let mut seen: HashSet<u64> = HashSet::new();
            let mut presets: Vec<PresetEntry> = Vec::with_capacity(instances.len());
            for (i, inst) in instances.iter().enumerate() {
                let hash = hash_bytes(&inst.chunk.chunk);
                let duplicate = !seen.insert(hash);
                let report = fxp::validate_chunk_report(&inst.chunk.chunk);
                presets.push(PresetEntry {
                    index: i as u32,
                    channel: inst.channel,
                    channel_name: inst.channel_name.clone(),
                    plugin_name: inst.plugin_name.clone(),
                    preset_name: inst.chunk.meta.preset_name.clone(),
                    author: inst.chunk.meta.author.clone(),
                    category: inst.chunk.meta.category.clone(),
                    version_f32: inst.chunk.meta.version_f32,
                    state_bytes: inst.chunk.stream_sizes.first().copied().unwrap_or(0),
                    chunk_bytes: inst.chunk.chunk.len(),
                    source: format!("{:?}", inst.chunk.source),
                    duplicate,
                    content_hash: hash.to_string(),
                    valid: report.is_ok(),
                    warnings: report.warnings().map(str::to_string).collect(),
                    errors: report.fatals().map(str::to_string).collect(),
                });
                out.line(&format!(
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
                ));
            }
            input_reports.push(ListInputReport {
                input: doc.name.clone(),
                presets,
                failed: stats.failed,
                serum2_skipped: stats.serum2_count as u32,
            });
        }
    }
    if total_presets == 0 {
        eprintln!("no Serum presets found in any input");
    }
    Ok(ListReport {
        inputs: input_reports,
        preset_count: total_presets as u32,
    })
}

fn run_validate(inputs: &[PathBuf], out: &Out) -> Result<ValidateReport, String> {
    let mut input_reports: Vec<ValidateInputReport> = Vec::new();
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
        out.line(&format!("{}: {}", input.display(), status));
        let mut errors: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        for f in report.fatals() {
            all_ok = false;
            errors.push(f.to_string());
            out.line(&format!("  error: {f}"));
        }
        for w in report.warnings() {
            warnings.push(w.to_string());
            out.line(&format!("  note: {w}"));
        }
        let mut preset_name = None;
        if report.is_ok() {
            // informational: decompress and show the embedded preset name
            if let Ok((state, _)) = fxp::inflate_state(&data)
                && state.len() > serum::OFF_PRESET_NAME
            {
                let name = String::from_utf8_lossy(
                    &state[serum::OFF_PRESET_NAME..serum::OFF_PRESET_NAME + 32],
                );
                let name = name.trim_end_matches('\0').trim();
                out.line(&format!("  preset: {name}"));
                preset_name = Some(name.to_string());
            }
        }
        input_reports.push(ValidateInputReport {
            input: input.display().to_string(),
            valid: report.is_ok(),
            errors,
            warnings,
            preset_name,
        });
    }
    let mut report = ValidateReport {
        inputs: input_reports,
        error: None,
    };
    if !all_ok {
        report.error = Some("validation failed".into());
    }
    Ok(report)
}

fn run_convert(
    inputs: &[PathBuf],
    out_path_opt: Option<&PathBuf>,
    dry_run: bool,
    out: &Out,
) -> Result<ConvertReport, String> {
    if out_path_opt.is_some() && inputs.len() > 1 {
        return Err("--out can only be used with a single input file".into());
    }
    let mut source = flpconv::RealSource::embedded();
    let mut input_reports: Vec<ConvertInputReport> = Vec::new();
    let mut total_converted = 0usize;
    for input in inputs {
        let docs = read_input_docs(input)?;
        if out_path_opt.is_some() && docs.len() > 1 {
            return Err(format!(
                "--out cannot be used with '{}' (the archive holds {} .flp member(s); \
                 extract the members or convert them one file at a time)",
                input.display(),
                docs.len()
            ));
        }
        // Unique default output names span the members of one archive.
        let mut used_out_names: HashMap<String, usize> = HashMap::new();
        for doc in &docs {
            // Single walk: the plans already carry each instance's parsed
            // Serum preset (plan.s1) and preset name; nothing re-scans the
            // buffer.
            let (plans, scan_warnings) = match flpconv::scan_convertible_detailed(&doc.data) {
                Ok(v) => v,
                Err(e) => {
                    if doc.from_archive {
                        eprintln!("warning: skipping {}: {e}", doc.name);
                        continue;
                    }
                    return Err(e);
                }
            };
            if plans.is_empty() {
                for w in &scan_warnings {
                    print_warning(doc, w);
                }
                eprintln!("{}: no convertible Serum instances found", doc.name);
                input_reports.push(ConvertInputReport {
                    input: doc.name.clone(),
                    dry_run,
                    output: None,
                    output_bytes: None,
                    converted_count: 0,
                    details: Vec::new(),
                    warnings: scan_warnings,
                });
                continue;
            }
            out.line(&format!(
                "{}: converting {} Serum instance(s)",
                doc.name,
                plans.len()
            ));
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
            let (out_buf, report) = flpconv::apply(&doc.data, &plans, &bundles)?;
            let mut details: Vec<ConvertDetail> = Vec::with_capacity(report.converted.len());
            for (k, c) in report.converted.iter().enumerate() {
                details.push(ConvertDetail {
                    channel: c.channel,
                    channel_name: c.channel_name.clone(),
                    preset_name: c.preset_name.clone(),
                    payload_len: c.new_payload_len,
                    notes: Vec::new(),
                });
                let state_bytes = plans[k]
                    .s1
                    .as_ref()
                    .map(|p| p.blob.len())
                    .unwrap_or_default();
                out.line(&format!(
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
                ));
            }
            let converted_count = report.converted.len();
            total_converted += converted_count;
            let mut warnings = scan_warnings;
            warnings.extend(report.warnings.iter().cloned());
            for w in &warnings {
                print_warning(doc, w);
            }
            if dry_run {
                out.line("dry run: no files written");
                input_reports.push(ConvertInputReport {
                    input: doc.name.clone(),
                    dry_run: true,
                    output: None,
                    output_bytes: None,
                    converted_count: converted_count as u32,
                    details,
                    warnings,
                });
                continue;
            }
            let out_path = match out_path_opt {
                Some(o) => o.clone(),
                None if doc.from_archive => {
                    let member = doc.name.rsplit('#').next().unwrap_or("member");
                    let member_stem = Path::new(member)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "member".into());
                    let input_stem = input
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "output".into());
                    let base = sanitize_filename(&member_stem);
                    let count = used_out_names.entry(base.clone()).or_insert(0);
                    *count += 1;
                    let name = if *count == 1 {
                        format!("{input_stem}_{base}_serum2.flp")
                    } else {
                        format!("{input_stem}_{base}_{}_serum2.flp", count)
                    };
                    input.with_file_name(name)
                }
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
            std::fs::write(&out_path, &out_buf)
                .map_err(|e| format!("{}: {e}", out_path.display()))?;
            out.line(&format!(
                "converted {} instance(s) -> {} ({} bytes)",
                converted_count,
                out_path.display(),
                out_buf.len()
            ));
            input_reports.push(ConvertInputReport {
                input: doc.name.clone(),
                dry_run: false,
                output: Some(out_path.display().to_string()),
                output_bytes: Some(out_buf.len()),
                converted_count: converted_count as u32,
                details,
                warnings,
            });
        }
    }
    let mut report = ConvertReport {
        inputs: input_reports,
        converted_count: total_converted as u32,
        error: None,
    };
    if total_converted == 0 {
        report.error = Some("no instances converted".into());
    }
    Ok(report)
}

/// Uniform wrapper over the four command reports so `main` can print any
/// success result without knowing its concrete type (`Serialize` is not
/// dyn-compatible, so this is an enum rather than a trait object).
enum AnyReport {
    List(ListReport),
    Extract(ExtractReport),
    Validate(ValidateReport),
    Convert(ConvertReport),
    Patch(PatchReport),
}

impl AnyReport {
    fn error(&self) -> Option<&str> {
        match self {
            AnyReport::List(r) => r.error(),
            AnyReport::Extract(r) => r.error(),
            AnyReport::Validate(r) => r.error(),
            AnyReport::Convert(r) => r.error(),
            AnyReport::Patch(r) => r.error(),
        }
    }
}

impl serde::Serialize for AnyReport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AnyReport::List(r) => r.serialize(serializer),
            AnyReport::Extract(r) => r.serialize(serializer),
            AnyReport::Validate(r) => r.serialize(serializer),
            AnyReport::Convert(r) => r.serialize(serializer),
            AnyReport::Patch(r) => r.serialize(serializer),
        }
    }
}

/// Write `data` to `path` atomically: temp file in the same directory +
/// rename over the target.
fn write_atomic(path: &std::path::Path, data: &[u8]) -> Result<(), String> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => std::path::PathBuf::from("."),
    };
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output".into()),
        std::process::id()
    ));
    std::fs::write(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}

/// NUL-terminated string at `off` inside the decompressed state (CLI display
/// helper; the library keeps `core::cstr` crate-private).
fn state_str(state: &[u8], off: usize, len: usize) -> String {
    let end = (off + len).min(state.len());
    String::from_utf8_lossy(&state[off..end])
        .trim_end_matches('\0')
        .trim()
        .to_string()
}

fn run_patch(
    input: &std::path::Path,
    name: Option<&str>,
    author: Option<&str>,
    category: Option<&str>,
    out: Option<&PathBuf>,
    dry_run: bool,
    report_out: &Out,
) -> Result<PatchReport, String> {
    let mut patches: Vec<fxp::PatchField> = Vec::new();
    if let Some(s) = name {
        patches.push(fxp::PatchField::Name(s.to_string()));
    }
    if let Some(s) = author {
        patches.push(fxp::PatchField::Author(s.to_string()));
    }
    if let Some(s) = category {
        patches.push(fxp::PatchField::Category(s.to_string()));
    }
    if patches.is_empty() {
        return Err("nothing to patch: pass --name and/or --author and/or --category".into());
    }
    let buf = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
    let is_flp = buf.len() >= 4 && &buf[0..4] == b"FLhd";
    if is_flp {
        let (output, count, warnings) = run_patch_flp(&buf, &patches, out, dry_run, report_out)?;
        Ok(PatchReport {
            input: input.display().to_string(),
            output,
            dry_run,
            patched: count,
            warnings,
        })
    } else {
        let (output, warnings) = run_patch_fxp(input, &buf, &patches, out, dry_run, report_out)?;
        Ok(PatchReport {
            input: input.display().to_string(),
            output,
            dry_run,
            patched: 1,
            warnings,
        })
    }
}

fn run_patch_fxp(
    input: &std::path::Path,
    buf: &[u8],
    patches: &[fxp::PatchField],
    out: Option<&PathBuf>,
    dry_run: bool,
    report_out: &Out,
) -> Result<(String, Vec<String>), String> {
    // Current values for the old -> new summary (best effort).
    let (state, _) = fxp::inflate_state(buf)
        .map_err(|e| format!("{}: cannot read the preset state: {e}", input.display()))?;
    let old = (
        state_str(&state, serum::OFF_PRESET_NAME, 32),
        state_str(&state, serum::OFF_AUTHOR, 48),
        state_str(&state, serum::OFF_CATEGORY, 48),
    );
    let old_header_name = String::from_utf8_lossy(&buf[0x1C..0x38])
        .trim_end_matches('\0')
        .to_string();

    let mut patched = buf.to_vec();
    fxp::patch_metadata(&mut patched, patches)?;
    report_out.line(&format!(
        "{}: patching {} field(s)",
        input.display(),
        patches.len()
    ));
    for p in patches {
        match p {
            fxp::PatchField::Name(s) => report_out.line(&format!(
                "  name: '{old_header_name}' / '{}' -> '{s}'",
                old.0
            )),
            fxp::PatchField::Author(s) => {
                report_out.line(&format!("  author: '{}' -> '{s}'", old.1))
            }
            fxp::PatchField::Category(s) => {
                report_out.line(&format!("  category: '{}' -> '{s}'", old.2))
            }
        }
    }

    // Validate the result with the same rules the importer applies.
    let report = fxp::validate_fxp(&patched);
    if !report.is_ok() {
        for f in report.fatals() {
            eprintln!("  error: {f}");
        }
        return Err("patched file fails Serum2 validation; not written".into());
    }
    let warnings: Vec<String> = report.warnings().map(|w| w.to_string()).collect();
    for w in &warnings {
        report_out.line(&format!("  note: {w}"));
    }
    report_out.line("  validate: PASS");

    if dry_run {
        report_out.line("dry run: no files written");
        return Ok((String::from("<dry-run>"), warnings));
    }
    let target = out
        .map(|p| (*p).clone())
        .unwrap_or_else(|| input.to_path_buf());
    write_atomic(&target, &patched)?;
    report_out.line(&format!(
        "patched {} -> {} ({} bytes)",
        input.display(),
        target.display(),
        patched.len()
    ));
    Ok((target.display().to_string(), warnings))
}

fn run_patch_flp(
    buf: &[u8],
    patches: &[fxp::PatchField],
    out: Option<&PathBuf>,
    dry_run: bool,
    report_out: &Out,
) -> Result<(String, u32, Vec<String>), String> {
    let (patched, report) = flpconv::patch_serum_metadata(buf, patches)?;
    report_out.line(&format!(
        "patching {} Serum instance(s)",
        report.patched.len()
    ));
    for p in &report.patched {
        report_out.line(&format!(
            "  channel '{}' preset '{}' -> '{}'",
            if p.channel_name.is_empty() {
                "-"
            } else {
                &p.channel_name
            },
            if p.old_preset_name.is_empty() {
                "-"
            } else {
                &p.old_preset_name
            },
            if p.new_preset_name.is_empty() {
                "-"
            } else {
                &p.new_preset_name
            },
        ));
    }
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    if report.patched.is_empty() {
        return Err("no Serum instances patched".into());
    }
    if dry_run {
        report_out.line("dry run: no files written");
        return Ok((
            String::from("<dry-run>"),
            report.patched.len() as u32,
            report.warnings,
        ));
    }
    // The patched FLP path must be given explicitly: never rewrite a project
    // file in place by accident.
    let Some(target) = out else {
        return Err(
            "patching an FLP requires --out (refusing to rewrite the project in place)".into(),
        );
    };
    write_atomic(target, &patched)?;
    report_out.line(&format!(
        "patched {} instance(s) -> {} ({} bytes)",
        report.patched.len(),
        target.display(),
        patched.len()
    ));
    Ok((
        target.display().to_string(),
        report.patched.len() as u32,
        report.warnings,
    ))
}

/// Convert standalone Serum .fxp presets into Serum2 .SerumPreset files.
/// Abort-on-error semantics like `run_convert`: the first input that fails
/// validation or conversion stops the run with an error.
fn run_convert_fxp(
    inputs: &[PathBuf],
    out: Option<&PathBuf>,
    overwrite: bool,
    report_out: &Out,
) -> Result<ConvertReport, String> {
    let mut total = 0u32;
    let mut input_reports: Vec<ConvertInputReport> = Vec::new();
    for (k, input) in inputs.iter().enumerate() {
        let buf = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
        let c =
            flpconv::convert_fxp_bytes(&buf).map_err(|e| format!("{}: {e}", input.display()))?;
        for n in &c.notes {
            report_out.line(&format!("warning: {n}"));
        }
        let out_dir = match out {
            Some(o) => o.clone(),
            None => input
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".")),
        };
        std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
        let stem = input
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "preset".into());
        let path = out_dir.join(format!("{}.SerumPreset", sanitize_filename(&stem)));
        if path.exists() && !overwrite {
            report_out.line(&format!(
                "{} exists, skipped (use --overwrite)",
                path.display()
            ));
            input_reports.push(ConvertInputReport {
                input: input.display().to_string(),
                dry_run: false,
                output: None,
                output_bytes: None,
                converted_count: 0,
                details: Vec::new(),
                warnings: c.notes.clone(),
            });
            continue;
        }
        std::fs::write(&path, &c.serum_preset).map_err(|e| format!("{}: {e}", path.display()))?;
        total += 1;
        report_out.line(&format!(
            "[{:02}] preset '{}' (state {}, {} stream(s), ver {:.4}) -> converted (body {} B) => {}",
            k + 1,
            if c.preset_name.is_empty() { "-" } else { &c.preset_name },
            format_bytes(c.state_size),
            c.stream_count,
            c.version_f32,
            c.body_cbor_len,
            path.display(),
        ));
        input_reports.push(ConvertInputReport {
            input: input.display().to_string(),
            dry_run: false,
            output: Some(path.display().to_string()),
            output_bytes: Some(c.serum_preset.len()),
            converted_count: 1,
            details: Vec::new(),
            warnings: c.notes.clone(),
        });
    }
    if total == 0 {
        return Err("no presets converted".into());
    }
    Ok(ConvertReport {
        inputs: input_reports,
        converted_count: total,
        error: None,
    })
}
fn main() {
    let cli = Cli::parse();
    let json = match &cli.command {
        Command::List { json, .. }
        | Command::Extract { json, .. }
        | Command::Validate { json, .. }
        | Command::Convert { json, .. }
        | Command::ConvertFxp { json, .. }
        | Command::Patch { json, .. } => *json,
    };
    let out = Out { json };
    let result: Result<AnyReport, String> = match &cli.command {
        Command::List { inputs, .. } => run_list(inputs, &out).map(AnyReport::List),
        Command::Extract {
            inputs,
            out: out_dir,
            overwrite,
            keep_invalid,
            keep_duplicates,
            ..
        } => run_extract(
            inputs,
            out_dir.as_ref(),
            *overwrite,
            *keep_invalid,
            *keep_duplicates,
            &out,
        )
        .map(AnyReport::Extract),
        Command::Validate { inputs, .. } => run_validate(inputs, &out).map(AnyReport::Validate),
        Command::Convert {
            inputs,
            out: out_path,
            dry_run,
            ..
        } => run_convert(inputs, out_path.as_ref(), *dry_run, &out).map(AnyReport::Convert),
        Command::ConvertFxp {
            inputs,
            out: fxp_out,
            overwrite,
            json: _,
        } => run_convert_fxp(inputs, fxp_out.as_ref(), *overwrite, &out).map(AnyReport::Convert),
        Command::Patch {
            input,
            name,
            author,
            category,
            out: patch_out,
            dry_run,
            json: _,
        } => run_patch(
            input,
            name.as_deref(),
            author.as_deref(),
            category.as_deref(),
            patch_out.as_ref(),
            *dry_run,
            &out,
        )
        .map(AnyReport::Patch),
    };
    let mut code = 0;
    match result {
        Ok(report) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("report serializes to JSON")
                );
            }
            if let Some(e) = report.error() {
                if !json {
                    eprintln!("error: {e}");
                }
                code = 1;
            }
        }
        Err(e) => {
            if json {
                println!("{}", serde_json::json!({ "error": e }));
            } else {
                eprintln!("error: {e}");
            }
            code = 1;
        }
    }
    if code != 0 {
        std::process::exit(1);
    }
}
