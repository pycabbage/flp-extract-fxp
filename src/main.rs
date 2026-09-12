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
};
use flp_extract_fxp::flpconv::BundleSource;
use flp_extract_fxp::{flpconv, fxp, serum};
use std::collections::HashMap;
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

/// True when a path string carries glob syntax (`*` or `?`). A literal Unix
/// filename containing those characters would be treated as a pattern; the
/// expansion then simply finds nothing for it.
fn has_glob_syntax(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains('*') || s.contains('?')
}

/// Case-insensitive `.flp` extension check.
fn has_flp_ext(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("flp"))
}

fn is_sep(c: char) -> bool {
    c == '/' || c == std::path::MAIN_SEPARATOR
}

/// Resolves CLI input paths into a sorted, de-duplicated list of files.
///
/// - existing files pass through unchanged,
/// - directories are walked recursively with `std::fs` only, collecting
///   `*.flp` case-insensitively,
/// - glob patterns (`*`/`?` per path component, `**` across components) are
///   expanded in-process; matches are filtered to `.flp` like directory
///   walks (these commands only consume FLP files),
/// - symlinks are never followed, so directory cycles are impossible.
///
/// An input that resolves to nothing (empty directory, unmatched pattern,
/// missing path) aborts the whole command with
/// `no .flp files found in <path>`.
fn resolve_inputs(inputs: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let mut resolved: Vec<PathBuf> = Vec::new();
    for input in inputs {
        let matches = if has_glob_syntax(&input) {
            expand_glob(&input)?
        } else if input.is_dir() {
            let mut found = Vec::new();
            collect_flps(&input, &mut found)?;
            found
        } else if input.exists() {
            vec![input.clone()]
        } else {
            Vec::new()
        };
        if matches.is_empty() {
            return Err(format!("no .flp files found in {}", input.display()));
        }
        resolved.extend(matches);
    }
    // Directory walks and glob expansion return entries in readdir order;
    // sort + dedup so output and multi-input processing are deterministic
    // and the same file is never processed twice.
    resolved.sort();
    resolved.dedup();
    Ok(resolved)
}

/// Recursively collects `.flp` files (case-insensitive) under `dir`.
fn collect_flps(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let ft = entry
            .file_type()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if ft.is_dir() {
            collect_flps(&path, out)?;
        } else if ft.is_file() && has_flp_ext(&path) {
            out.push(path);
        }
    }
    Ok(())
}

/// Expands a glob pattern against the filesystem.
///
/// The components before the first wildcard component form a literal base
/// directory; the rest is matched level by level (see [`expand_level`]). I/O
/// errors propagate, except a base that is missing or not a directory, which
/// simply yields no matches.
fn expand_glob(pattern: &Path) -> Result<Vec<PathBuf>, String> {
    let pat = pattern.to_string_lossy().to_string();
    let comps: Vec<&str> = pat.split(std::path::is_separator).collect();
    let Some(wild) = comps
        .iter()
        .position(|c| c.contains('*') || c.contains('?'))
    else {
        // Callers only get here with glob syntax present, so this is
        // unreachable; be graceful anyway.
        return Ok(Vec::new());
    };

    let mut base = PathBuf::new();
    if pat.starts_with('/') || pat.starts_with(std::path::MAIN_SEPARATOR) {
        base.push(std::path::MAIN_SEPARATOR.to_string());
    }
    for c in &comps[..wild] {
        if !c.is_empty() {
            base.push(c);
        }
    }
    if base.as_os_str().is_empty() {
        base.push(".");
    }
    if !base.is_dir() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    expand_level(&[base], &comps[wild..], &mut matches)?;
    Ok(matches)
}

/// Matches `comps` (starting at the first wildcard component) against the
/// candidate directories in `frontier`, appending matching `.flp` files to
/// `out`. Only the final pattern component matches files; earlier ones
/// descend into matching subdirectories.
fn expand_level(
    frontier: &[PathBuf],
    comps: &[&str],
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let Some((head, rest)) = comps.split_first() else {
        return Ok(());
    };
    if *head == "**" {
        // `**` matches zero or more directory levels: the next component is
        // tried against every frontier directory plus all (recursively
        // reachable) subdirectories of them.
        let mut dirs = frontier.to_vec();
        let mut i = 0;
        while i < dirs.len() {
            let mut subdirs = Vec::new();
            let entries =
                std::fs::read_dir(&dirs[i]).map_err(|e| format!("{}: {e}", dirs[i].display()))?;
            for entry in entries {
                let entry = entry.map_err(|e| format!("{}: {e}", dirs[i].display()))?;
                let path = entry.path();
                let is_dir = entry
                    .file_type()
                    .map_err(|e| format!("{}: {e}", path.display()))?
                    .is_dir();
                if is_dir {
                    subdirs.push(path);
                }
            }
            dirs.extend(subdirs);
            i += 1;
        }
        return expand_level(&dirs, rest, out);
    }
    let last = rest.is_empty();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in frontier {
        let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !match_component(head, name) {
                continue;
            }
            let path = entry.path();
            let ft = entry
                .file_type()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if last {
                if ft.is_file() && has_flp_ext(&path) {
                    out.push(path);
                }
            } else if ft.is_dir() {
                dirs.push(path);
            }
        }
    }
    if last {
        Ok(())
    } else {
        expand_level(&dirs, rest, out)
    }
}

/// Wildcard match of a single pattern component against a single path
/// component: `*` matches any run of characters, `?` exactly one; neither
/// crosses path separators. Matching is case-insensitive (ASCII), consistent
/// with the case-insensitive `.flp` collection of directory walks.
fn match_component(pat: &str, name: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi].eq_ignore_ascii_case(&n[ni]) || (p[pi] == '?' && !is_sep(n[ni])))
        {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            if is_sep(n[mark]) {
                return false;
            }
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
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
        let (instances, stats) = scan_serum_instances(&buf)?;
        for msg in &stats.failed {
            eprintln!("warning: {msg}");
        }
        if instances.is_empty() {
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
        Command::List { inputs } => resolve_inputs(inputs.to_vec()).and_then(|i| run_list(&i)),
        Command::Extract {
            inputs,
            out,
            overwrite,
            keep_invalid,
        } => resolve_inputs(inputs.to_vec())
            .and_then(|i| run_extract(&i, out.as_ref(), *overwrite, *keep_invalid)),
        Command::Validate { inputs } => run_validate(inputs),
        Command::Convert {
            inputs,
            out,
            dry_run,
        } => resolve_inputs(inputs.to_vec()).and_then(|i| run_convert(&i, out.as_ref(), *dry_run)),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pat: &str, path: &str) -> bool {
        match_components(
            &pat.split('/').collect::<Vec<_>>(),
            &path.split('/').collect::<Vec<_>>(),
        )
    }

    /// `/`-split pattern vs `/`-split path; `**` matches zero or more
    /// components. Test-only helper: the runtime path matches component by
    /// component in [`expand_level`].
    fn match_components(pat: &[&str], path: &[&str]) -> bool {
        let Some((head, rest)) = pat.split_first() else {
            return path.is_empty();
        };
        if *head == "**" {
            return (0..=path.len()).any(|skip| match_components(rest, &path[skip..]));
        }
        match path.split_first() {
            Some((head_p, rest_p)) => {
                match_component(head, head_p) && match_components(rest, rest_p)
            }
            None => false,
        }
    }

    #[test]
    fn component_wildcards() {
        assert!(match_component("*.flp", "song.flp"));
        assert!(!match_component("*.flp", "song.flpx"));
        assert!(match_component("s?ng", "song"));
        assert!(match_component("s?ng", "s.ng"));
        assert!(!match_component("s?ng", "sg"));
        assert!(match_component("*", ""));
        assert!(match_component("a*b*c", "a-x-b-y-c"));
        assert!(match_component("a*b*c", "abc"));
        assert!(!match_component("a*b*c", "a-b-c-d"));
        // `*`/`?` never cross path separators.
        assert!(!match_component("*", "a/b"));
        assert!(!match_component("a*b", "a/x/b"));
    }

    #[test]
    fn component_matching_is_case_insensitive() {
        assert!(match_component("*.FLP", "x.FLP"));
        assert!(match_component("*.FLP", "x.flp"));
        assert!(match_component("*.flp", "x.FLP"));
        assert!(match_component("B?", "ba"));
        assert!(!match_component("ba", "BA?"));
    }

    #[test]
    fn double_star_matches_zero_or_more_components() {
        assert!(m("**/*.flp", "a.flp"));
        assert!(m("**/*.flp", "sub/a.flp"));
        assert!(m("**/*.flp", "x/y/z/a.flp"));
        assert!(!m("**/*.flp", "a.txt"));
        assert!(m("a/**/b.flp", "a/b.flp"));
        assert!(m("a/**/b.flp", "a/s/t/b.flp"));
        assert!(!m("a/**/b.flp", "b.flp"));
        // A plain `*` component stays within one level.
        assert!(m("*.flp", "a.flp"));
        assert!(!m("*.flp", "sub/a.flp"));
        assert!(!m("a/*.flp", "a.flp"));
    }

    #[test]
    fn flp_extension_case_insensitive() {
        assert!(has_flp_ext(Path::new("x.flp")));
        assert!(has_flp_ext(Path::new("dir/x.FLP")));
        assert!(has_flp_ext(Path::new("x.Flp")));
        assert!(!has_flp_ext(Path::new("x.flpx")));
        assert!(!has_flp_ext(Path::new("flp")));
    }

    #[test]
    fn glob_syntax_detection() {
        assert!(has_glob_syntax(Path::new("dir/**/*.flp")));
        assert!(has_glob_syntax(Path::new("?.flp")));
        assert!(!has_glob_syntax(Path::new("plain.flp")));
    }
}
