## Description
Provide a brief summary of the changes made in this Pull Request and the issue or feature it resolves.

## Type of Change
- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update
- [ ] Packaging / CI update

## Verification Checklist
- [ ] Code compiles with `cargo check` (0 errors, 0 warnings)
- [ ] Automated tests pass with `cargo test --verbose` (0 failures)
- [ ] Linter passes with `cargo clippy -- -D warnings`
- [ ] Code formatted with `cargo fmt --check`
- [ ] Preserves all domain invariants (minimum 10.0% ROI, hauler limits, home-inclusive jumps, safe routing)
- [ ] Desktop / AppStream metadata validated if packaging files were modified
