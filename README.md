# cleanrs

cleanrs is a macOS- and Windows-focused, rule-based disk cleanup tool. The first milestone
implements a read-only scan and command-backed cleanup for npm, Homebrew, pip,
uv, Yarn, pnpm, Cargo, Docker, and common coding-agent caches (Codex, Claude
Code, Kiro CLI, Ollama, and other known cache locations).

The core now has explicit `platform/macos` and `platform/windows` configuration
boundaries. The next `v0.1.7` release includes macOS and Windows binaries.

Install the Windows binary without Rust or Homebrew from PowerShell:

    irm https://raw.githubusercontent.com/ho-doan/my-cleaner/master/scripts/install.ps1 | iex

It installs a checksum-verified `cleanrs.exe` into the user profile and updates
the user PATH. The installer supports Windows x64 and Windows ARM64 through
the built-in x64 emulation layer, and waits for the release checksum manifest
to become available before installing. It is compatible with Windows PowerShell
5.1 and PowerShell 7.

## Usage

    cargo run -p cleanrs -- list
    cargo run -p cleanrs -- list --readonly
    cargo run -p cleanrs -- scan
    cargo run -p cleanrs -- scan --full-disk
    cargo run -p cleanrs -- tui
    cargo run -p cleanrs -- scan --only npm,brew --json
    cargo run -p cleanrs -- clean --only npm
    cargo run -p cleanrs -- clean --only npm --yes
    cargo run -p cleanrs -- clean --only npm --yes --permanent
    cargo run -p cleanrs -- clean --only docker --yes --force
    cargo run -p cleanrs -- scan --only codex,claude,kiro,ollama,agent-caches

## Install with Homebrew

The repository includes a Homebrew formula that installs the prebuilt
architecture-specific binary from the latest versioned GitHub release:

    brew tap ho-doan/my-cleaner https://github.com/ho-doan/my-cleaner.git
    brew install ho-doan/my-cleaner/cleanrs
    cleanrs tui

The formula verifies the release checksum and installs the cleanrs binary into
the normal Homebrew prefix; Rust is not required on the user's machine.

## Install with curl (when Homebrew requires newer Xcode tools)

If Homebrew blocks the install because the Command Line Tools are outdated,
use the user-local installer instead. It downloads the architecture-specific
prebuilt binary, verifies its SHA-256 checksum, and does not require Rust,
Homebrew, Xcode, or `sudo`:

    curl -fsSL https://raw.githubusercontent.com/ho-doan/my-cleaner/master/scripts/install.sh | sh
    export PATH="$HOME/.local/bin:$PATH"
    cleanrs tui

The installer supports Apple Silicon and Intel Macs. Set
`CLEANRS_INSTALL_DIR` if a different user-writable installation directory is
needed.

## Upgrade

The TUI checks the latest GitHub release in the background each time it opens.
When a newer verified release is available, the Version panel shows the
current/latest versions and the footer enables `u`. Pressing `u` closes the
TUI first, then upgrades through the detected installation channel:

- Homebrew installs run `brew update` and `brew upgrade`.
- curl installs rerun the HTTPS installer with the verified latest version; the
  installer waits and retries while GitHub's release metadata and checksum map
  propagate through the CDN.

If the network check fails, cleanup remains usable and no upgrade is attempted.
There is no automatic upgrade.

For a manual upgrade, use the same channel used for installation:

    brew update
    brew upgrade ho-doan/my-cleaner/cleanrs

For the curl installer, run the installer again; it replaces only the existing
user-local binary after verifying the release checksum:

    curl -fsSL https://raw.githubusercontent.com/ho-doan/my-cleaner/master/scripts/install.sh | sh

Verify the installed version with `cleanrs --version`. The installer supports
`CLEANRS_VERSION` for a released version whose checksum is published.

## Uninstall

For the curl installer, remove only the cleanrs binary:

    rm -f "$HOME/.local/bin/cleanrs"

If you added `$HOME/.local/bin` to `.zshrc`, remove that PATH line separately.
Do not remove the whole directory because it may contain other user binaries.

For Homebrew:

    brew uninstall ho-doan/my-cleaner/cleanrs

Removing the tap is optional:

    brew untap ho-doan/my-cleaner

## License

cleanrs is released under the [MIT License](LICENSE).

The clean command is a preview by default. Cleaning never runs unless --yes is
present. Rules invoke the package manager's own cleanup command instead of
deleting its internal files directly.

`--permanent` is an additional explicit opt-in for targets that normally move
to Trash. It requires `--yes`, permanently deletes only path-based targets, and
does not change the behavior of package-manager commands. The TUI always uses
Trash-safe deletion.

## User-configured rules

Copy `config/default_rules.toml` to
`~/.config/cleanrs/rules.toml` to add custom path rules. Paths must be absolute
or use `~/`. Path rules move data to Trash by default. Optional commands are
argv arrays (not shell strings) and are Manual-risk unless explicitly marked
otherwise; they never go through a shell.

Every executed cleanup result and global-tool uninstall is appended to
`~/.cleanrs/history.log` as one JSON object per line; dry-run remains read-only.
History logging is an audit trail separate from `tracing`, is best-effort, and
never blocks a successful cleanup. Set `RUST_LOG=cleanrs=debug` for diagnostic
scan/clean spans. cleanrs is synchronous at the application layer and uses
Rayon/crossbeam for parallel work; Tokio should only be reintroduced in an
isolated network module for features such as update checks or remote rules.

Xcode Archives, old Xcode.app installations, old macOS installer applications,
unavailable simulator devices, Mail downloads, iOS backups, and the Docker
Desktop VM image are exposed as Manual/Caution review targets. Old Xcode apps
are checked in /Applications and ~/Applications; the active xcode-select -p
app is excluded. macOS installer apps are checked in /Applications,
~/Applications, ~/Downloads, ~/Desktop, and ~/Documents. CoreSimulator
generated caches are Caution targets. /Library is inventory-only and readonly;
/macOS Install Data and APFS system/update data are never turned into
unrestricted delete targets.

Interactive cleanup prompts are handled by the rule. For example, Dart's
`pub cache clean` receives an explicit `y` only after the user confirms execute
mode; cleanup processes never inherit the TUI's stdin.

Docker cleanup is Manual-risk because the official prune command also removes
unused volumes; it requires both --yes and --force.

When a supported macOS app has been removed but its user data remains, the
`Uninstalled app leftovers` rule reports the known data paths as separate
Manual-review targets. It currently covers Docker Desktop, Visual Studio Code,
Cursor (including versioned `ShipIt` updater caches), Postman, Slack, and
Discord. The rule only activates when the corresponding `.app` bundle is
absent, uses Trash-safe path cleanup, and never guesses at arbitrary
`~/Library` directories. Docker's container data, including `Docker.raw`, is
therefore still visible after Docker Desktop is uninstalled without being
silently treated as a normal cache.

Agent session/history data and Ollama models are treated as Manual-risk. Ollama
models are removed through the official ollama rm command, never by deleting
the model store directly. Active Codex and Claude runtimes are preserved.

The TUI starts in dry-run mode; press d to switch to execute mode. Pressing y
in dry-run previews only, keeps the targets visible, and does not pretend that
files were removed. In execute mode, Safe and Caution targets can be cleaned;
Manual targets require the explicit FORCE confirmation. After an actual clean,
the TUI shows background progress per target, then rescans and refreshes the
disk snapshot. Press r to reload manually.
For long-running command-backed cleaners such as `uv cache clean`, the TUI
streams command output when available and shows the active target, estimated
size, elapsed time, and heartbeat while the worker is still running. The
underlying command remains the package manager's official cleanup command.

Press f for an inventory of the largest root-disk and HOME directories; b
returns to cleanup targets. From the inventory, press Enter on a folder to scan
its children in the background; use Enter again to drill down and b to go back.
Git repositories show clean/dirty status, and regenerable directories such as
`node_modules`, `target`, `.dart_tool`, and build caches are marked as
`SUGGEST`. `CoreSimulator` and app/config data remain `REVIEW`: they can be
inspected but are not whole-folder delete targets. Protected system/mount roots
are shown as `READONLY` with their recursively calculated size, and can be
opened for inspection. Full-disk inventory still never creates delete targets;
cleanup requires an explicit explorer selection and confirmation. Inside a
directory, select a non-protected file, a safe `SUGGEST` folder, a core dump, or
an individual system-temp file under `/private/tmp` or `/private/var/tmp`, then
press x to move it to Trash. System roots, mounted volumes, databases, swap,
and sensitive folders remain review-only.

For an exact directory that needs explicit review, press `w` in the Explorer
to toggle it in the delete allowlist at
`~/.config/cleanrs/whitelist.toml`. Allowlisting only makes that exact
directory an approved suggestion; it never deletes anything by itself, and
`x` plus the Trash confirmation are still required. Press `w` again to remove
the path from the allowlist.

Items inside `~/Downloads` are treated as explicit user-approved cleanup
candidates: files and folders can be moved to Trash with `x` and confirmation.
The `Downloads` directory itself is never auto-approved, and cleanrs never
selects or deletes Downloads items in bulk.

Press g to open the global-tools inventory. It checks Cargo-installed binaries,
Dart pub global, npm, pnpm, Yarn, uv, pipx, and Homebrew formula/cask when the
corresponding command is available. Select a tool and press x or Enter to
review the exact official uninstall command; y runs it in the background and
the inventory reloads automatically. cleanrs never deletes a package-manager
directory directly, and npm itself is shown as `KEEP` because it manages its
own installation.

Press s to open the separate standalone-install inventory. It checks direct
executable entries in common user-owned binary roots such as ~/.local/bin,
~/bin, ~/.deno/bin, ~/.bun/bin, and ~/.volta/bin. This is intended for tools
installed by a curl/bootstrap script or a direct user installer when no package
manager can report provenance. Each item shows its exact path and size; cleanrs
only moves that one file or symlink to Trash after confirmation, never the
whole root directory. It also reports known version stores such as Codex
standalone releases and Claude Code versions: active versions are KEEP, while
old direct child folders can be moved to Trash after confirmation. The scan
cannot prove installer provenance for generic binaries, so review the path
before removal. The currently running cleanrs binary and active CLI versions
are protected.

Homebrew casks may require administrator authentication: run sudo -v in a
separate Terminal first. cleanrs checks cached authentication without
collecting or storing the password, so a missing credential fails clearly
instead of leaving the TUI waiting on a hidden prompt.

The Trash row is deliberately separate from bulk selection and is marked
`DESTRUCTIVE`. In the TUI press `t`, then type `EMPTY TRASH` exactly and press
Enter; `y`, `--yes`, and `--force` never bypass this confirmation. The CLI
equivalent is `--confirm-destructive="EMPTY TRASH"`; use it only when the
explicit Finder empty-trash action is intended. There is no scheduled or cron
variant of this action.

`/Library/Updates`, macOS Install Data, and `/System/Volumes/*` are report-only:
they show their recursively calculated size and advice, but have no `clean`
method, no selectable checkbox, and are skipped by `clean --all --yes` with an
explicit read-only summary. Use `list --readonly` to inspect that inventory.
