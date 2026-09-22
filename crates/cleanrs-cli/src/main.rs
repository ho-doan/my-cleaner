use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use cleanrs_core::{
    all_cleaners, full_disk_scan, scan_all, CleanResult, Cleaner, CleanerScan, FullDiskScan,
};
use comfy_table::{presets::UTF8_FULL, Table};
use humansize::{format_size, DECIMAL};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Parser)]
#[command(name = "cleanrs", version, about = "Safe, rule-based disk cleanup")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scan available cleaners without changing files.
    Scan(ScanArgs),
    /// Preview or execute cleaning for selected cleaners.
    Clean(CleanArgs),
    /// List registered cleaners and whether their command is available.
    List,
    /// Open the interactive terminal UI.
    Tui,
}

#[derive(Debug, Args)]
struct ScanArgs {
    /// Comma-separated cleaner IDs, for example: npm,brew.
    #[arg(long, value_delimiter = ',')]
    only: Option<Vec<String>>,
    /// Inventory the largest top-level directories on the root disk and HOME.
    /// This is read-only and does not create delete targets.
    #[arg(long)]
    full_disk: bool,
    /// Number of largest entries to show per scanned location.
    #[arg(long, default_value_t = 12)]
    top: usize,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CleanArgs {
    /// Comma-separated cleaner IDs, for example: npm.
    #[arg(long, value_delimiter = ',')]
    only: Option<Vec<String>>,
    /// Explicitly keep this operation as a preview.
    #[arg(long)]
    dry_run: bool,
    /// Execute commands. Without this flag clean is always a dry-run.
    #[arg(long)]
    yes: bool,
    /// Allow execution of Manual-risk rules, such as Docker volume pruning.
    #[arg(long)]
    force: bool,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct CleanOutput {
    dry_run: bool,
    results: Vec<CleanResult>,
    errors: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan(args) => scan_command(args),
        Command::Clean(args) => clean_command(args),
        Command::List => list_command(),
        Command::Tui => cleanrs_tui::run(),
    }
}

fn scan_command(args: ScanArgs) -> Result<()> {
    if args.top == 0 {
        bail!("--top must be greater than zero");
    }

    if args.full_disk {
        if args.only.is_some() {
            bail!("--full-disk cannot be combined with --only");
        }
        let report = full_disk_scan(Path::new("/"), args.top)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_full_disk_table(&report);
        }
        return Ok(());
    }

    if let Some(ids) = args.only.as_deref() {
        selected_cleaners(Some(ids))?;
    }
    let reports = scan_all(args.only.as_deref());
    if args.json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        print_scan_table(&reports);
    }
    Ok(())
}

fn clean_command(args: CleanArgs) -> Result<()> {
    let dry_run = !args.yes || args.dry_run;
    let selected = selected_cleaners(args.only.as_deref())?;
    let mut results = Vec::new();
    let mut errors = Vec::new();

    for cleaner in selected {
        if !cleaner.is_available() {
            continue;
        }

        if !dry_run && cleaner.risk_level() == cleanrs_core::RiskLevel::Manual && !args.force {
            errors.push(format!(
                "{} requires --force because it is a Manual-risk cleaner",
                cleaner.id()
            ));
            continue;
        }

        let targets = match cleaner.scan() {
            Ok(targets) => targets,
            Err(error) => {
                errors.push(format!("{}: {error:#}", cleaner.id()));
                continue;
            }
        };

        for target in targets {
            match cleaner.clean(&target, dry_run) {
                Ok(result) => results.push(result),
                Err(error) => errors.push(format!("{}: {error:#}", cleaner.id())),
            }
        }
    }

    let output = CleanOutput {
        dry_run,
        results,
        errors,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_clean_output(&output);
    }

    if !output.errors.is_empty() {
        bail!("one or more cleaners failed");
    }
    Ok(())
}

fn list_command() -> Result<()> {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(["ID", "Cleaner", "Category", "Risk", "Available"]);

    for cleaner in all_cleaners() {
        table.add_row([
            cleaner.id().to_owned(),
            cleaner.display_name().to_owned(),
            format!("{:?}", cleaner.category()),
            format!("{:?}", cleaner.risk_level()),
            cleaner.is_available().to_string(),
        ]);
    }

    println!("{table}");
    Ok(())
}

fn selected_cleaners(only: Option<&[String]>) -> Result<Vec<Box<dyn Cleaner>>> {
    let cleaners = all_cleaners();
    let Some(ids) = only else {
        return Ok(cleaners);
    };

    let normalized = ids
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let mut selected = Vec::new();
    let mut found = std::collections::HashSet::new();

    for cleaner in cleaners {
        if normalized.contains(cleaner.id()) {
            found.insert(cleaner.id().to_owned());
            selected.push(cleaner);
        }
    }

    let unknown = normalized.difference(&found).cloned().collect::<Vec<_>>();
    if !unknown.is_empty() {
        bail!("unknown cleaner(s): {}", unknown.join(", "));
    }
    Ok(selected)
}

fn print_scan_table(reports: &[CleanerScan]) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(["Cleaner", "Target", "Size", "Risk", "Path"]);

    for report in reports {
        if let Some(error) = &report.error {
            table.add_row([
                report.display_name.clone(),
                "scan error".to_owned(),
                "-".to_owned(),
                format!("{:?}", report.risk_level),
                error.clone(),
            ]);
        } else if !report.available {
            table.add_row([
                report.display_name.clone(),
                "unavailable".to_owned(),
                "-".to_owned(),
                format!("{:?}", report.risk_level),
                "-".to_owned(),
            ]);
        } else if report.targets.is_empty() {
            table.add_row([
                report.display_name.clone(),
                "nothing to clean".to_owned(),
                "0 B".to_owned(),
                format!("{:?}", report.risk_level),
                "-".to_owned(),
            ]);
        } else {
            for target in &report.targets {
                table.add_row([
                    report.display_name.clone(),
                    target.description.clone(),
                    format_size(target.size_bytes, DECIMAL),
                    format!("{:?}", report.risk_level),
                    target.path.display().to_string(),
                ]);
            }
        }
    }

    println!("{table}");
}

fn print_full_disk_table(report: &FullDiskScan) {
    println!("Full-disk inventory (read-only; no delete targets generated)");
    println!(
        "Read-only mount/system paths: {}",
        report.readonly_paths.len()
    );
    println!("Inaccessible paths: {}", report.inaccessible_paths);

    for (label, entries) in [
        ("Root disk", &report.root_entries),
        ("HOME", &report.home_entries),
    ] {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header([label, "State", "Size", "Path"]);
        if entries.is_empty() {
            table.add_row(["-", "READONLY", "0 B", "No readable entries"]);
        } else {
            for entry in entries {
                table.add_row([
                    label.to_owned(),
                    if entry.read_only { "READONLY" } else { "-" }.to_owned(),
                    format_size(entry.size_bytes, DECIMAL),
                    entry.path.display().to_string(),
                ]);
            }
        }
        println!("{table}");
    }

    if !report.readonly_paths.is_empty() {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(["State", "Reason", "Path"]);
        for path in &report.readonly_paths {
            table.add_row([
                "READONLY".to_owned(),
                "system/mount excluded".to_owned(),
                path.display().to_string(),
            ]);
        }
        println!("{table}");
    }
}

fn print_clean_output(output: &CleanOutput) {
    if output.dry_run {
        println!("Dry-run: no command was executed.");
    }

    for result in &output.results {
        let status = if result.dry_run { "preview" } else { "cleaned" };
        println!(
            "[{status}] {} — {} ({})",
            result.cleaner_id,
            result.target,
            format_size(result.expected_freed_bytes, DECIMAL)
        );
    }

    for error in &output.errors {
        eprintln!("[error] {error}");
    }
}
