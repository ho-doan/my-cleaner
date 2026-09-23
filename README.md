# cleanrs

cleanrs is a macOS-focused, rule-based disk cleanup tool. The first milestone
implements a read-only scan and command-backed cleanup for npm, Homebrew, pip,
uv, Yarn, pnpm, Cargo, Docker, and common coding-agent caches (Codex, Claude
Code, Kiro CLI, Ollama, and other known cache locations).

## Usage

    cargo run -p cleanrs -- list
    cargo run -p cleanrs -- scan
    cargo run -p cleanrs -- scan --full-disk
    cargo run -p cleanrs -- tui
    cargo run -p cleanrs -- scan --only npm,brew --json
    cargo run -p cleanrs -- clean --only npm
    cargo run -p cleanrs -- clean --only npm --yes
    cargo run -p cleanrs -- clean --only docker --yes --force
    cargo run -p cleanrs -- scan --only codex,claude,kiro,ollama,agent-caches

## Install with Homebrew

The repository includes a Homebrew formula that builds cleanrs from the
versioned source tag:

    brew tap ho-doan/my-cleaner https://github.com/ho-doan/my-cleaner.git
    brew install ho-doan/my-cleaner/cleanrs
    cleanrs tui

The formula uses Homebrew's Rust build dependency and installs the cleanrs
binary into the normal Homebrew prefix.

The clean command is a preview by default. Cleaning never runs unless --yes is
present. Rules invoke the package manager's own cleanup command instead of
deleting its internal files directly.

Interactive cleanup prompts are handled by the rule. For example, Dart's
`pub cache clean` receives an explicit `y` only after the user confirms execute
mode; cleanup processes never inherit the TUI's stdin.

Docker cleanup is Manual-risk because the official prune command also removes
unused volumes; it requires both --yes and --force.

Agent session/history data and Ollama models are treated as Manual-risk. Ollama
models are removed through the official ollama rm command, never by deleting
the model store directly. Active Codex and Claude runtimes are preserved.

The TUI starts in dry-run mode; press d to switch to execute mode. Pressing y
in dry-run previews only, keeps the targets visible, and does not pretend that
files were removed. In execute mode, Safe and Caution targets can be cleaned;
Manual targets require the explicit FORCE confirmation. After an actual clean,
the TUI shows background progress per target, then rescans and refreshes the
disk snapshot. Press r to reload manually.

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

Press g to open the global-tools inventory. It checks Cargo-installed binaries,
Dart pub global, npm, pnpm, Yarn, uv, pipx, and Homebrew formula/cask when the
corresponding command is available. Select a tool and press x or Enter to
review the exact official uninstall command; y runs it in the background and
the inventory reloads automatically. cleanrs never deletes a package-manager
directory directly, and npm itself is shown as `KEEP` because it manages its
own installation.

Homebrew casks may require administrator authentication: run sudo -v in a
separate Terminal first. cleanrs checks cached authentication without
collecting or storing the password, so a missing credential fails clearly
instead of leaving the TUI waiting on a hidden prompt.
