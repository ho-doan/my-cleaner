use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use cleanrs_core::history::record_history;
use cleanrs_core::{
    all_cleaners, default_scan_root, full_disk_scan, scan_all, scan_all_reports, CleanOptions,
    CleanResult, Cleaner, CleanerScan, FullDiskScan, ReadOnlyScan, RiskLevel,
};
use comfy_table::{presets::UTF8_FULL, Table};
use humansize::{format_size, DECIMAL};
use serde::Serialize;

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
    List(ListArgs),
    /// Open the interactive terminal UI.
    Tui,
}

#[derive(Debug, Args)]
struct ListArgs {
    /// List report-only system paths instead of cleaners.
    #[arg(long)]
    readonly: bool,
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
    /// Explicitly select every cleaner, including the destructive Trash row.
    #[arg(long, conflicts_with = "only")]
    all: bool,
    /// Explicitly keep this operation as a preview.
    #[arg(long)]
    dry_run: bool,
    /// Execute commands. Without this flag clean is always a dry-run.
    #[arg(long)]
    yes: bool,
    /// Allow execution of Manual-risk rules, such as Docker volume pruning.
    #[arg(long)]
    force: bool,
    /// Permanently delete TrashPath targets instead of moving them to Trash.
    /// Requires --yes and does not alter manager-owned commands.
    #[arg(long, requires = "yes", conflicts_with = "dry_run")]
    permanent: bool,
    /// Exact confirmation required for Empty Trash.
    #[arg(long, value_name = "EMPTY TRASH")]
    confirm_destructive: Option<String>,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct CleanOutput {
    dry_run: bool,
    results: Vec<CleanResult>,
    errors: Vec<String>,
    skipped_readonly_count: usize,
    skipped_readonly_bytes: u64,
}

#[derive(Debug, Serialize)]
struct ScanOutput {
    cleaners: Vec<CleanerScan>,
    readonly: Vec<ReadOnlyScan>,
}

fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    match cli.command {
        Command::Scan(args) => scan_command(args),
        Command::Clean(args) => clean_command(args),
        Command::List(args) => list_command(args),
        Command::Tui => match cleanrs_tui::run(env!("CARGO_PKG_VERSION"))? {
            cleanrs_tui::RunOutcome::Exit => Ok(()),
            cleanrs_tui::RunOutcome::Upgrade(update) => cleanrs_core::perform_upgrade(&update),
        },
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("cleanrs=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init();
}

fn scan_command(args: ScanArgs) -> Result<()> {
    if args.top == 0 {
        bail!("--top must be greater than zero");
    }

    if args.full_disk {
        if args.only.is_some() {
            bail!("--full-disk cannot be combined with --only");
        }
        let root = default_scan_root();
        let report = full_disk_scan(&root, args.top)?;
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
    let readonly = scan_all_reports();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ScanOutput {
                cleaners: reports,
                readonly
            })?
        );
    } else {
        print_scan_table(&reports);
        print_readonly_table(&readonly);
    }
    Ok(())
}

fn clean_command(args: CleanArgs) -> Result<()> {
    let dry_run = !args.yes || args.dry_run;
    let selected = selected_cleaners(args.only.as_deref())?;
    let all_mode = args.all || args.only.is_none();
    let (skipped_readonly_count, skipped_readonly_bytes) = if all_mode {
        let reports = scan_all_reports();
        let targets = reports
            .into_iter()
            .flat_map(|report| report.targets)
            .collect::<Vec<_>>();
        if !dry_run {
            for target in &targets {
                record_history(
                    "clean",
                    "report-only",
                    &target.path.display().to_string(),
                    "skipped_readonly",
                    &target.description,
                );
            }
        }
        (
            targets.len(),
            targets.iter().map(|target| target.size_bytes).sum(),
        )
    } else {
        (0, 0)
    };
    let mut results = Vec::new();
    let mut errors = Vec::new();

    for cleaner in selected {
        if !cleaner.is_available() {
            continue;
        }

        if cleaner.risk_level() == RiskLevel::Destructive
            && args.confirm_destructive.as_deref() != Some("EMPTY TRASH")
        {
            errors.push(format!(
                "{} skipped: requires --confirm-destructive=\"EMPTY TRASH\"; --yes/--force are insufficient",
                cleaner.id()
            ));
            continue;
        }

        if !dry_run && cleaner.risk_level() == RiskLevel::Manual && !args.force {
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
            match cleaner.clean_with_options(
                &target,
                CleanOptions {
                    dry_run,
                    permanent: args.permanent,
                },
            ) {
                Ok(result) => results.push(result),
                Err(error) => errors.push(format!("{}: {error:#}", cleaner.id())),
            }
        }
    }

    let output = CleanOutput {
        dry_run,
        results,
        errors,
        skipped_readonly_count,
        skipped_readonly_bytes,
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

fn list_command(args: ListArgs) -> Result<()> {
    if args.readonly {
        print_readonly_table(&scan_all_reports());
        return Ok(());
    }

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

fn print_readonly_table(reports: &[ReadOnlyScan]) {
    println!("Read-only reports (never eligible for clean):");
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(["Report", "State", "Size", "Path", "Advice"]);
    let mut rows = 0;
    for report in reports {
        if let Some(error) = &report.error {
            table.add_row([
                report.report_id.clone(),
                "ERROR".to_owned(),
                "-".to_owned(),
                error.clone(),
                report.advice.clone(),
            ]);
            rows += 1;
        }
        for target in &report.targets {
            table.add_row([
                format!("🔒 {}", report.report_id),
                "READONLY".to_owned(),
                format_size(target.size_bytes, DECIMAL),
                target.path.display().to_string(),
                report.advice.clone(),
            ]);
            rows += 1;
        }
    }
    if rows == 0 {
        table.add_row([
            "-".to_owned(),
            "READONLY".to_owned(),
            "0 B".to_owned(),
            "No report-only targets found".to_owned(),
            "System paths remain protected".to_owned(),
        ]);
    }
    println!("{table}");
}

fn print_full_disk_table(report: &FullDiskScan) {
    println!("Full-disk inventory (read-only; no delete targets generated)");
    println!(
        "Read-only mount/system paths: {}",
        report.readonly_entries.len()
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

    if !report.readonly_entries.is_empty() {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(["State", "Reason", "Size", "Path"]);
        for entry in &report.readonly_entries {
            table.add_row([
                "READONLY".to_owned(),
                "system/mount excluded".to_owned(),
                format_size(entry.size_bytes, DECIMAL),
                entry.path.display().to_string(),
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

    if output.skipped_readonly_count > 0 {
        println!(
            "[skip] Skipped {} read-only targets ({})",
            output.skipped_readonly_count,
            format_size(output.skipped_readonly_bytes, DECIMAL)
        );
    }
}
