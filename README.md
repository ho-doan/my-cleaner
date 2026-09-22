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

The clean command is a preview by default. Cleaning never runs unless --yes is
present. Rules invoke the package manager's own cleanup command instead of
deleting its internal files directly.

Docker cleanup is Manual-risk because the official prune command also removes
unused volumes; it requires both --yes and --force.

Agent session/history data and Ollama models are treated as Manual-risk. Ollama
models are removed through the official ollama rm command, never by deleting
the model store directly. Active Codex and Claude runtimes are preserved.

The TUI shows root-disk Used/Free/Total and a usage gauge. Press r to reload a
scan manually; after a clean confirmation it automatically rescans and refreshes
the disk snapshot. Press f for a read-only inventory of the largest root-disk
and HOME directories; b returns to cleanup targets. Full-disk inventory never
creates delete targets.
