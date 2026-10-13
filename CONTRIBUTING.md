# Contributing to EVE Traveling Merchant

Thank you for your interest in contributing to **EVE Traveling Merchant**! Whether you are an EVE Online trading veteran, a Rust developer, or an open-source contributor, your help is appreciated.

---

## Code of Conduct

All contributors and participants are expected to adhere to our [Code of Conduct](CODE_OF_CONDUCT.md). Please treat everyone with respect and kindness.

---

## Development Setup

### 1. Prerequisites
- **Rust 1.78+** (installed via [rustup](https://rustup.rs/))
- **C Compiler & System Headers**:
  - Fedora: `sudo dnf install gcc pkg-config git -y`
  - Ubuntu/Debian: `sudo apt install build-essential pkg-config git -y`
  - Arch: `sudo pacman -S base-devel git`

### 2. Clone the Repository
```bash
git clone https://github.com/Flambello/eve-traveling-merchant.git
cd eve-traveling-merchant
```

### 3. Build & Test
```bash
# Verify type checking and compilation
make check
# or: cargo check

# Run the automated test suites
make test
# or: cargo test --verbose

# Run the native desktop GUI
cargo run --bin eve-merchant -- --gui
```

---

## Core Domain Invariants

When adding features or refactoring, you must preserve these key domain invariants:
1. **Minimum Net Profitability (ROI)**: All arbitrage opportunities must enforce a minimum of **10.0%** net return on capital after broker fees and sales taxes.
2. **Ship Profile Constraints**:
   - **Big Hauler**: Strictly capped at `60,000 m³` volume and `500,000,000 ISK` capital budget.
   - **Small Hauler**: Strictly capped at `30,000 m³` volume and `50,000,000 ISK` capital budget.
3. **True Profit-Per-Jump Ranking**: Every candidate deal must include travel jumps from the pilot's starting location to the source station in the total jump denominator (`total_jumps = jumps_from_home + jumps_source_dest`).
4. **Safe Highsec Routing**: Stargate routing must avoid lowsec and nullsec systems (security < 0.45) by default unless explicitly disabled by the user.

---

## Testing & Quality Assurance Protocol

**Every pull request must pass the automated test suites with zero failures and zero compiler warnings:**

```bash
# 1. Compile & Typecheck
cargo check --all-targets

# 2. Run Test Suite
cargo test --verbose

# 3. Linter
cargo clippy -- -D warnings

# 4. Formatter
cargo fmt --check

# 5. Desktop Entry Verification
desktop-file-validate eve-trade.desktop
```

### Writing New Tests
Whenever introducing new trading algorithms, filters, or route optimizers, include corresponding integration tests in `tests/`.

---

## Submitting Pull Requests

1. **Fork the repository** and create a feature branch (`git checkout -b feature/my-cool-filter`).
2. **Commit your changes** using clear commit messages following conventional commits (`feat: ...`, `fix: ...`, `docs: ...`, `test: ...`).
3. **Push to your fork** and submit a Pull Request to `main`.
4. Our GitHub Actions CI will automatically test and validate your PR.
