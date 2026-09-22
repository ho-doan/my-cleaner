# cleanrs

cleanrs is a macOS-focused, rule-based disk cleanup tool. The first milestone
implements a read-only scan and safe command-backed cleanup for npm, Homebrew,
and pip caches.

## Usage

    cargo run -p cleanrs -- list
    cargo run -p cleanrs -- scan
    cargo run -p cleanrs -- scan --only npm,brew --json
    cargo run -p cleanrs -- clean --only npm
    cargo run -p cleanrs -- clean --only npm --yes

The clean command is a preview by default. Cleaning never runs unless --yes is
present. Rules invoke the package manager's own cleanup command instead of
deleting its internal files directly.

