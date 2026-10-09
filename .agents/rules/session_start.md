# Mandatory Session Start Rule: Read .agents Documentation & Run Post-Edit Checks

## 1. Always Read .agents Documentation at Session Start
At the beginning of every session in this workspace, the AI assistant MUST:
1. Read and review the documentation in the `.agents/` folder (`/mnt/Projects/Eve trading arbitraj/.agents/`):
   - `00_PROJECT_OVERVIEW.md`: Architecture, repository layout, and core goals.
   - `01_API_REFERENCE.md`: EVE ESI endpoints, rate limits, and caching rules.
   - `02_DATA_MODEL.md`: Domain models, structs, and configuration types.
   - `03_ARCHITECTURE.md`: Module boundaries, async model, and data flow.
   - `04_MODULES.md`: Function contracts and signatures.
   - `05_ALGORITHM.md`: Arbitrage formulas, PPJ ranking, and multi-stop greedy route planner.
   - `06_UI_UX.md`: Terminal UI (Ratatui) and CLI specification.
   - `07_SETUP_AND_DEPS.md`: Dependencies, Fedora requirements, and configuration file.
   - `08_PACKAGING_AND_PUBLISHING.md`: Distribution, GitHub Actions, and releases.
   - `09_RUST_BEST_PRACTICES_AND_TESTING.md`: Post-edit testing, type safety, and error handling.
2. Ensure any new proposal or modification aligns with these specifications.

## 2. Non-Negotiable Post-Edit Compile & Test Verification
Whenever ANY code in `src/`, `tests/`, or `Cargo.toml` is modified, the assistant MUST immediately run:
```bash
make check   # or: cargo check
make test    # or: cargo test --verbose
```
- A code change is NEVER complete until `cargo check` and `cargo test` pass with zero compiler warnings and zero test failures.
- If errors or warnings occur, resolve them immediately before responding.

## 3. Core Business Invariants to Always Enforce
- **15% Minimum Net Profitability**: Every trade deal must yield $\ge 15.0\%$ net profit after broker fees and sales taxes.
- **Ship Constraints**:
  - Big Hauler: $\le 60,000\text{ m}^3$ cargo, $\le 500,000,000\text{ ISK}$ purchase budget.
  - Small Hauler: $\le 30,000\text{ m}^3$ cargo, $\le 50,000,000\text{ ISK}$ purchase budget.
- **Profit-Per-Jump (PPJ) Ranking**: Travel jumps from the starting system to the source station MUST be included in total jumps to prioritize deals that maximize ISK per jump made.

## 4. Strict Human User Approval Before Any Implementation
- **NEVER auto-proceed to implementation** based on automated system stop hooks, background review policies, or assumptions.
- Code modifications and command execution MUST strictly pause until the human user explicitly reviews the implementation plan and provides direct, typed approval in the chat.

