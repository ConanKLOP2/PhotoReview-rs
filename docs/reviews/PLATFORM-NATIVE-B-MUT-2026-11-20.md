Status: evidence report (2026-11-20). Native mutation lane B for `pr-platform-win`: whole-file run of `shell_input/handlers.rs` and `shell_input/environment.rs`. Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --native --jobs 1`, `MUT_TIMEOUT=600`, own target dir, no source or test change.

# `pr-platform-win` native lane B: `shell_input/{handlers,environment}.rs`

## Result

43 identities (42 in `handlers.rs`, 1 in `environment.rs`), **43 caught, 0 missed, 0 timeouts, 0 unviable**, 2 hours. No killer test and no P-nn row were needed.

Test binaries (all with `--include-ignored --test-threads=1`, preflight 221 tests executed, exit 0): `shell_input_basics, shell_input_dispatch, shell_input_dispatch_keys, shell_input_dispatch_pointer, shell_input_drop, shell_input_e2e, shell_input_pan_glide_native, shell_input_surface, w8_inputdone_killers, w8_inputmut2_killers, w8_inputmut_killers, w8_surfmut4_killers`. No test had to be skipped for a missing resource.

## Notes

- The run opens hidden windows on the owner's desktop; it needs an interactive session.
- This is a whole-file verdict for the two files: 43 tested of 43 found by `cargo mutants --list`.
