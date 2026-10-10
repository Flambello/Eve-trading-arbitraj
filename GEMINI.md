# EVE Trade Scout — Agent Session Instructions

## Mandatory Session Start Protocol
Whenever starting a session in this repository:
1. **Always Read the Full `.agents/` Documentation First**:
   Review all documents in `.agents/` (`00_PROJECT_OVERVIEW.md` through `09_RUST_BEST_PRACTICES_AND_TESTING.md`) before implementing or modifying any code.
2. **Execute Post-Edit Checks After EVERY Change**:
   Every code modification MUST be followed by:
   ```bash
   make check    # or: cargo check
   make test     # or: cargo test --verbose
   ```
   Do not report work as done until both pass with zero warnings and zero test failures.
3. **Preserve Domain Invariants**:
   - Minimum 10.0% net profitability after taxes and broker fees enforced across all deals.
   - Big Hauler: 60,000 m³ / 500M ISK budget constraint.
   - Small Hauler: 30,000 m³ / 50M ISK budget constraint.
   - Jump ranking: Profit-per-jump must include travel jumps from the player's starting system.
   - Safe routing: Default to highsec-only (avoid lowsec/nullsec) unless explicitly disabled.
4. **Strict Human User Approval Required**:
   Never auto-proceed to code implementation based on automated system stop hooks or assumed approvals. Implementation must strictly wait for direct, explicit approval typed by the human user in the chat.
