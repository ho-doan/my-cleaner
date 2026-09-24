//! View model and state owned by the TUI.

use cleanrs_core::{
    Category, CleanResult, CleanTarget, CleanerScan, DirectoryScan, DirectoryScanEntry,
    FullDiskScan, GlobalTool, GlobalToolResult, GlobalToolScan, ReadOnlyScan, RiskLevel,
    StandaloneTool, StandaloneToolResult, StandaloneToolScan, UpdateInfo,
};
use crossbeam_channel::Receiver;
use fs2::{available_space, total_space};
use std::time::Instant;

pub(crate) struct TargetRow {
    pub(crate) cleaner_id: String,
    pub(crate) cleaner_name: String,
    pub(crate) category: Category,
    pub(crate) risk: RiskLevel,
    pub(crate) target: CleanTarget,
    pub(crate) selected: bool,
}

pub(crate) enum Mode {
    Scanning,
    Reviewing,
    Confirming,
    Cleaning,
}

pub(crate) enum UpdateState {
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Unavailable(String),
}

#[derive(Clone, Copy)]
pub(crate) struct DiskUsage {
    pub(crate) total_bytes: u64,
    pub(crate) free_bytes: u64,
}

impl DiskUsage {
    pub(crate) fn used_bytes(self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    pub(crate) fn used_ratio(self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.used_bytes() as f64 / self.total_bytes as f64
        }
    }
}

pub(crate) enum CleanMessage {
    Started {
        cleaner_id: String,
        target: CleanTarget,
    },
    Progress(String),
    Target(Result<CleanResult, String>),
    Finished,
}

pub(crate) struct App {
    pub(crate) mode: Mode,
    pub(crate) current_version: String,
    pub(crate) update_state: UpdateState,
    pub(crate) rows: Vec<TargetRow>,
    pub(crate) trash_target: Option<CleanTarget>,
    pub(crate) trash_selected: bool,
    pub(crate) readonly_report: Option<ReadOnlyScan>,
    pub(crate) info_overlay: Option<String>,
    pub(crate) pending_empty_trash: bool,
    pub(crate) cursor: usize,
    pub(crate) dry_run: bool,
    pub(crate) confirm_text: String,
    pub(crate) received_scans: usize,
    pub(crate) expected_scans: usize,
    pub(crate) results: Vec<CleanResult>,
    pub(crate) errors: Vec<String>,
    pub(crate) disk: Option<DiskUsage>,
    pub(crate) full_disk: Option<FullDiskScan>,
    pub(crate) full_disk_scanning: bool,
    pub(crate) full_disk_error: Option<String>,
    pub(crate) show_full_disk: bool,
    pub(crate) full_disk_cursor: usize,
    pub(crate) full_disk_phase: String,
    pub(crate) full_disk_completed: usize,
    pub(crate) full_disk_total: usize,
    pub(crate) show_global_tools: bool,
    pub(crate) global_tools: Option<GlobalToolScan>,
    pub(crate) global_tools_scanning: bool,
    pub(crate) global_tools_error: Option<String>,
    pub(crate) global_cursor: usize,
    pub(crate) pending_global_uninstall: Option<GlobalTool>,
    pub(crate) global_uninstall_receiver: Option<Receiver<Result<GlobalToolResult, String>>>,
    pub(crate) global_uninstalling: bool,
    pub(crate) show_standalone_tools: bool,
    pub(crate) standalone_tools: Option<StandaloneToolScan>,
    pub(crate) standalone_tools_scanning: bool,
    pub(crate) standalone_tools_error: Option<String>,
    pub(crate) standalone_cursor: usize,
    pub(crate) pending_standalone_uninstall: Option<StandaloneTool>,
    pub(crate) standalone_uninstall_receiver:
        Option<Receiver<Result<StandaloneToolResult, String>>>,
    pub(crate) standalone_uninstalling: bool,
    pub(crate) directory_scan: Option<DirectoryScan>,
    pub(crate) directory_scanning: bool,
    pub(crate) directory_error: Option<String>,
    pub(crate) directory_history: Vec<std::path::PathBuf>,
    pub(crate) directory_cursor: usize,
    pub(crate) pending_explorer_delete: Option<CleanTarget>,
    pub(crate) explorer_delete_receiver: Option<Receiver<Result<CleanResult, String>>>,
    pub(crate) explorer_deleting: bool,
    pub(crate) clean_receiver: Option<Receiver<CleanMessage>>,
    pub(crate) cleaning_completed: usize,
    pub(crate) cleaning_total: usize,
    pub(crate) cleaning_disk_before: Option<DiskUsage>,
    pub(crate) cleaning_started_at: Option<Instant>,
    pub(crate) active_cleaner: Option<String>,
    pub(crate) active_target: Option<String>,
    pub(crate) active_target_size: u64,
    pub(crate) active_detail: Option<String>,
    pub(crate) last_action: Option<String>,
}

impl App {
    pub(crate) fn new(expected_scans: usize, current_version: &str) -> Self {
        Self {
            mode: Mode::Scanning,
            current_version: current_version.to_owned(),
            update_state: UpdateState::Checking,
            rows: Vec::new(),
            trash_target: None,
            trash_selected: false,
            readonly_report: None,
            info_overlay: None,
            pending_empty_trash: false,
            cursor: 0,
            dry_run: true,
            confirm_text: String::new(),
            received_scans: 0,
            expected_scans,
            results: Vec::new(),
            errors: Vec::new(),
            disk: read_disk_usage(),
            full_disk: None,
            full_disk_scanning: false,
            full_disk_error: None,
            show_full_disk: false,
            full_disk_cursor: 0,
            full_disk_phase: String::new(),
            full_disk_completed: 0,
            full_disk_total: 0,
            show_global_tools: false,
            global_tools: None,
            global_tools_scanning: false,
            global_tools_error: None,
            global_cursor: 0,
            pending_global_uninstall: None,
            global_uninstall_receiver: None,
            global_uninstalling: false,
            show_standalone_tools: false,
            standalone_tools: None,
            standalone_tools_scanning: false,
            standalone_tools_error: None,
            standalone_cursor: 0,
            pending_standalone_uninstall: None,
            standalone_uninstall_receiver: None,
            standalone_uninstalling: false,
            directory_scan: None,
            directory_scanning: false,
            directory_error: None,
            directory_history: Vec::new(),
            directory_cursor: 0,
            pending_explorer_delete: None,
            explorer_delete_receiver: None,
            explorer_deleting: false,
            clean_receiver: None,
            cleaning_completed: 0,
            cleaning_total: 0,
            cleaning_disk_before: None,
            cleaning_started_at: None,
            active_cleaner: None,
            active_target: None,
            active_target_size: 0,
            active_detail: None,
            last_action: None,
        }
    }

    pub(crate) fn reset_for_scan(&mut self, expected_scans: usize) {
        self.mode = Mode::Scanning;
        self.rows.clear();
        self.trash_target = None;
        self.trash_selected = false;
        self.readonly_report = None;
        self.info_overlay = None;
        self.pending_empty_trash = false;
        self.cursor = 0;
        self.received_scans = 0;
        self.expected_scans = expected_scans;
        self.confirm_text.clear();
        self.show_full_disk = false;
        self.full_disk_cursor = 0;
        self.full_disk_phase.clear();
        self.full_disk_completed = 0;
        self.full_disk_total = 0;
        self.show_global_tools = false;
        self.global_tools = None;
        self.global_tools_scanning = false;
        self.global_tools_error = None;
        self.global_cursor = 0;
        self.pending_global_uninstall = None;
        self.global_uninstall_receiver = None;
        self.global_uninstalling = false;
        self.show_standalone_tools = false;
        self.standalone_tools = None;
        self.standalone_tools_scanning = false;
        self.standalone_tools_error = None;
        self.standalone_cursor = 0;
        self.pending_standalone_uninstall = None;
        self.standalone_uninstall_receiver = None;
        self.standalone_uninstalling = false;
        self.directory_scan = None;
        self.directory_scanning = false;
        self.directory_error = None;
        self.directory_history.clear();
        self.directory_cursor = 0;
        self.pending_explorer_delete = None;
        self.explorer_delete_receiver = None;
        self.explorer_deleting = false;
        self.clean_receiver = None;
        self.cleaning_completed = 0;
        self.cleaning_total = 0;
        self.cleaning_disk_before = None;
        self.cleaning_started_at = None;
        self.active_cleaner = None;
        self.active_target = None;
        self.active_target_size = 0;
        self.active_detail = None;
        self.disk = read_disk_usage().or(self.disk);
    }

    pub(crate) fn available_update(&self) -> Option<UpdateInfo> {
        match &self.update_state {
            UpdateState::Available(update) => Some(update.clone()),
            _ => None,
        }
    }

    pub(crate) fn add_scan(&mut self, report: CleanerScan) {
        self.received_scans += 1;
        if report.risk_level == RiskLevel::Destructive {
            self.trash_target = report.targets.into_iter().next();
            self.trash_selected = false;
            return;
        }
        self.rows
            .extend(report.targets.into_iter().map(|target| TargetRow {
                cleaner_id: report.cleaner_id.clone(),
                cleaner_name: report.display_name.clone(),
                category: report.category,
                risk: report.risk_level,
                target,
                selected: report.risk_level == RiskLevel::Safe,
            }));
        self.rows.sort_by(|left, right| {
            right
                .target
                .size_bytes
                .cmp(&left.target.size_bytes)
                .then_with(|| left.target.path.cmp(&right.target.path))
                .then_with(|| left.cleaner_name.cmp(&right.cleaner_name))
        });
    }

    pub(crate) fn selected_count(&self) -> usize {
        self.rows.iter().filter(|row| row.selected).count()
            + usize::from(self.trash_selected && self.trash_target.is_some())
    }

    pub(crate) fn selected_size(&self) -> u64 {
        let regular = self
            .rows
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.target.size_bytes)
            .sum::<u64>();
        regular
            + self
                .trash_target
                .as_ref()
                .filter(|_| self.trash_selected)
                .map(|target| target.size_bytes)
                .unwrap_or_default()
    }

    pub(crate) fn has_manual_selection(&self) -> bool {
        !self.dry_run
            && self
                .rows
                .iter()
                .any(|row| row.selected && row.risk == RiskLevel::Manual)
    }

    pub(crate) fn toggle_current(&mut self) {
        if let Some(row) = self.rows.get_mut(self.cursor) {
            row.selected = !row.selected;
        }
    }

    pub(crate) fn toggle_trash(&mut self) {
        if self.trash_target.is_some() {
            self.trash_selected = !self.trash_selected;
        }
    }

    pub(crate) fn select_all_safe(&mut self) {
        for row in &mut self.rows {
            if row.risk == RiskLevel::Safe {
                row.selected = true;
            }
        }
    }

    pub(crate) fn move_cursor(&mut self, delta: i32) {
        if self.rows.is_empty() {
            return;
        }
        let max = self.rows.len() - 1;
        self.cursor = if delta.is_negative() {
            self.cursor.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (self.cursor + delta as usize).min(max)
        };
    }

    pub(crate) fn full_disk_entries(&self) -> Vec<cleanrs_core::DiskScanEntry> {
        self.full_disk
            .as_ref()
            .map(|report| {
                report
                    .root_entries
                    .iter()
                    .chain(report.home_entries.iter())
                    .chain(report.readonly_entries.iter())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn directory_entries(&self) -> &[DirectoryScanEntry] {
        self.directory_scan
            .as_ref()
            .map(|scan| scan.entries.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn global_tools(&self) -> &[GlobalTool] {
        self.global_tools
            .as_ref()
            .map(|scan| scan.tools.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn move_global_cursor(&mut self, delta: i32) {
        let len = self.global_tools().len();
        if len == 0 {
            return;
        }
        let max = len - 1;
        self.global_cursor = if delta.is_negative() {
            self.global_cursor
                .saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (self.global_cursor + delta as usize).min(max)
        };
    }

    pub(crate) fn standalone_tools(&self) -> &[StandaloneTool] {
        self.standalone_tools
            .as_ref()
            .map(|scan| scan.tools.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn move_standalone_cursor(&mut self, delta: i32) {
        let len = self.standalone_tools().len();
        if len == 0 {
            return;
        }
        let max = len - 1;
        self.standalone_cursor = if delta.is_negative() {
            self.standalone_cursor
                .saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (self.standalone_cursor + delta as usize).min(max)
        };
    }

    pub(crate) fn move_directory_cursor(&mut self, delta: i32) {
        let len = if self.directory_scan.is_some() {
            self.directory_entries().len()
        } else {
            self.full_disk_entries().len()
        };
        if len == 0 {
            return;
        }
        let max = len - 1;
        self.directory_cursor = if delta.is_negative() {
            self.directory_cursor
                .saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (self.directory_cursor + delta as usize).min(max)
        };
        self.full_disk_cursor = self.directory_cursor;
    }
}

pub(crate) fn read_disk_usage() -> Option<DiskUsage> {
    let total_bytes = total_space("/").ok()?;
    let free_bytes = available_space("/").ok()?;
    Some(DiskUsage {
        total_bytes,
        free_bytes,
    })
}
