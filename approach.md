# Kiến trúc tool dọn dẹp ổ cứng bằng Rust (CLI + TUI)

## 1. Stack công nghệ đề xuất

| Thành phần | Crate |
| --- | --- |
| CLI parsing | `clap` (derive API) |
| TUI framework | `ratatui` + `crossterm` |
| Async runtime | `tokio` (cho scan song song, I/O không block UI) |
| Duyệt file nhanh | `jwalk` hoặc `ignore` (song song hoá, dùng trong ripgrep) |
| Tính dung lượng | tự viết trên `std::fs::metadata` + song song qua `rayon` |
| Serialize config/rule | `serde` + `toml` hoặc `yaml` |
| Xoá an toàn | `trash` crate (đưa vào Thùng rác thay vì xoá cứng) |
| Logging | `tracing` |
| Progress/size format | `humansize`, `indicatif` (cho chế độ CLI thuần) |

## 2. Nguyên tắc thiết kế cốt lõi

1. **Không bao giờ xoá trực tiếp mặc định** — luôn có `--dry-run` mặc định bật, người dùng phải xác nhận hoặc dùng `--yes`/`--force`.
2. **Rule-based, không hardcode logic xoá** — mỗi "cleaner" (npm, brew, docker...) chỉ là một *rule definition* (đường dẫn, cách tính an toàn, cách xác nhận), để dễ mở rộng và người dùng có thể tự thêm rule qua file config.
3. **Scan trước, xoá sau, tách biệt hoàn toàn 2 pha** — pha 1 chỉ đọc (an toàn tuyệt đối), pha 2 mới ghi.
4. **Ưu tiên gọi lệnh chính chủ khi có thể** (vd `npm cache clean`, `docker system prune`, `brew cleanup`, `pip cache purge`) thay vì tự ý xoá thư mục nội bộ của tool khác — tránh làm hỏng metadata mà công cụ gốc quản lý.

## 3. Cấu trúc project

```
cleanrs/
├── Cargo.toml
├── crates/
│   ├── cleanrs-core/        # logic scan/clean thuần, không phụ thuộc UI
│   │   ├── scanner.rs
│   │   ├── rule.rs          # trait Cleaner
│   │   ├── executor.rs      # xoá / gọi command
│   │   └── rules/
│   │       ├── npm.rs
│   │       ├── brew.rs
│   │       ├── docker.rs
│   │       ├── pip.rs
│   │       ├── uv.rs
│   │       ├── dart_flutter.rs
│   │       ├── cargo.rs
│   │       ├── xcode.rs
│   │       ├── macos_system.rs   # ~/Library/Caches, /Library/Logs, macOS installer leftovers
│   │       └── mod.rs
│   ├── cleanrs-cli/         # clap subcommands, output text/json
│   └── cleanrs-tui/         # ratatui app
└── config/default_rules.toml
```

## 4. Model dữ liệu trung tâm

```rust
pub trait Cleaner: Send + Sync {
    fn id(&self) -> &'static str;              // "npm", "brew", "docker"
    fn display_name(&self) -> &'static str;
    fn category(&self) -> Category;             // PackageManager, SystemCache, Container, IDE...
    /// Chỉ đọc — trả về danh sách target kèm size, không đụng gì cả
    fn scan(&self) -> anyhow::Result<Vec<CleanTarget>>;
    /// An toàn để tự động xoá hay cần user duyệt riêng từng cái
    fn risk_level(&self) -> RiskLevel;           // Safe, Caution, Manual
    /// Ưu tiên gọi command gốc nếu có, fallback về xoá path
    fn clean(&self, target: &CleanTarget, dry_run: bool) -> anyhow::Result<CleanResult>;
    fn is_available(&self) -> bool;              // check binary tồn tại (which npm, which brew...)
}

pub struct CleanTarget {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub description: String,
    pub method: CleanMethod,   // DeleteDir | DeleteFile | RunCommand(Vec<String>)
}

pub enum RiskLevel { Safe, Caution, Manual }
```

Mỗi module trong `rules/` implement `Cleaner`. Ví dụ rút gọn:

```rust
// rules/npm.rs
impl Cleaner for NpmCleaner {
    fn scan(&self) -> anyhow::Result<Vec<CleanTarget>> {
        let cache_dir = npm_cache_path()?; // `npm config get cache`
        Ok(vec![CleanTarget {
            size_bytes: dir_size(&cache_dir)?,
            path: cache_dir,
            description: "npm global cache".into(),
            method: CleanMethod::RunCommand(vec!["npm".into(), "cache".into(), "clean".into(), "--force".into()]),
        }])
    }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Safe }
}
```

## 5. Danh sách rule cần cover (macOS-focused)

**Package/dependency managers**

- npm/yarn/pnpm cache (`~/.npm`, `~/Library/Caches/Yarn`, pnpm store)
- pip (`pip cache purge`, `~/Library/Caches/pip`)
- uv (`~/.cache/uv` hoặc `uv cache clean`)
- cargo (`~/.cargo/registry/cache`, `target/` trong các project — cần scan riêng vì rất tốn)
- brew (`brew cleanup -s`, `brew autoremove`, cache tại `~/Library/Caches/Homebrew`)
- dart/flutter (`flutter pub cache`, `.dart_tool/`, `build/` trong từng project)
- gradle (`~/.gradle/caches`), maven (`~/.m2`)

**Container/VM**

- Docker: `docker system df` để lấy số liệu, `docker system prune -af --volumes` (đánh dấu Manual/Caution vì xoá volume có thể mất data)
- Docker Desktop VM disk image (`~/Library/Containers/com.docker.docker`)

**macOS system**

- `~/Library/Caches/*` (theo từng app, hiển thị breakdown)
- `~/Library/Logs`, `/private/var/log`
- Xcode: DerivedData, Archives, simulator caches (`xcrun simctl delete unavailable`)
- macOS Installer leftovers (`/Library/Updates`, `macOS Install Data`)
- `~/Library/Application Support/CrashReporter`
- Trash đã xoá (`~/.Trash`)
- Mail downloads, iOS backups (`~/Library/Application Support/MobileSync/Backup`) — luôn Manual

**Scan song song** dùng `jwalk` + `rayon` để tính size nhanh trên các thư mục lớn (node_modules, DerivedData), có cache kết quả scan trong session để TUI redraw mượt.

## 6. CLI (clap subcommands)

```
cleanrs scan                     # quét tất cả, in bảng size theo category
cleanrs scan --only npm,docker
cleanrs clean --only npm --dry-run
cleanrs clean --only npm --yes
cleanrs list                     # liệt kê cleaner có sẵn, is_available()
cleanrs tui                      # mở giao diện TUI
```

Output CLI hỗ trợ `--json` để pipe vào jq/script khác.

## 7. TUI (ratatui) — layout đề xuất

```
┌─ Sidebar: Categories ──┬─ Main: Target list (checkbox) ──────┐
│ [x] Package Managers    │ ☑ npm cache          1.2 GB  Safe  │
│ [x] System Cache        │ ☑ brew cache         340 MB  Safe  │
│ [ ] Containers          │ ☐ docker images      8.4 GB  Caution│
│ [ ] Manual Review       │ ☐ Xcode DerivedData  5.1 GB  Safe  │
├──────────────────────────┴──────────────────────────────────┤
│ Total selected: 1.5 GB          [Space] toggle  [d] dry-run  │
│ Detail pane: path, last modified, command sẽ chạy            │
└────────────────────────────────────────────────────────────┘
  [Enter] Clean selected   [a] select all Safe   [q] quit
```

- State machine: `Scanning -> Reviewing -> Confirming -> Cleaning -> Done`
- Chạy `scan()` của từng cleaner trong `tokio::spawn` riêng, gửi kết quả qua channel để UI cập nhật progressive (không block).
- Panel confirm riêng cho `RiskLevel::Manual` (docker volumes, backups) — bắt gõ tên để xác nhận giống `rm -rf` guard.

## 8. An toàn & UX

- Luôn xoá qua `trash` crate trước, có flag `--permanent` mới xoá thật.
- Log mọi thao tác clean vào `~/.cleanrs/history.log` để có thể xem lại đã xoá gì, khi nào.
- Snapshot dung lượng trước/sau để hiển thị "Freed: 4.3 GB" cuối phiên.
- Rule config cho phép user override qua `~/.config/cleanrs/rules.toml` (thêm rule mới không cần recompile — dùng path pattern + optional shell command).

## 9. Roadmap triển khai

1. `cleanrs-core`: trait `Cleaner` + 3-4 rule đơn giản (npm, brew, pip) + `dir_size` song song.
2. `cleanrs-cli`: `scan`/`clean` chạy được, output bảng bằng `comfy-table`.
3. Thêm toàn bộ rule còn lại (docker, cargo, dart, xcode, macOS system).
4. `cleanrs-tui`: dựng skeleton ratatui, nối vào `cleanrs-core` qua channel.
5. Polish: trash-safe delete, history log, config override, `--json`.

---
