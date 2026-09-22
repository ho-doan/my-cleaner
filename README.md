# cleanrs

cleanrs is a macOS-focused, rule-based disk cleanup tool. The first milestone
implements a read-only scan and command-backed cleanup for npm, Homebrew, pip,
uv, Yarn, pnpm, Cargo, and Docker.

## Usage

    cargo run -p cleanrs -- list
    cargo run -p cleanrs -- scan
    cargo run -p cleanrs -- tui
    cargo run -p cleanrs -- scan --only npm,brew --json
    cargo run -p cleanrs -- clean --only npm
    cargo run -p cleanrs -- clean --only npm --yes
    cargo run -p cleanrs -- clean --only docker --yes --force

The clean command is a preview by default. Cleaning never runs unless --yes is
present. Rules invoke the package manager's own cleanup command instead of
deleting its internal files directly.

Docker cleanup is Manual-risk because the official prune command also removes
unused volumes; it requires both --yes and --force.
