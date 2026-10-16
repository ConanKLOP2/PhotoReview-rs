export const meta = {
  name: 'wave4-contracts',
  description: 'Write Wave 4 (imaging) contracts, stubs, oracle commands and goldens for all sub-units in docs/WAVE-4-PLAN.md, review the Tier B-D ones, wire, open one PR',
  phases: [{ title: 'Setup' }, { title: 'Write' }, { title: 'Wire' }],
}
const REPO = 'C:/MyProjects/PhotoReview-rs'
const WT = REPO + '/.claude/worktrees/w4-contracts'
const CS = 'C:/MyProjects/PhotoReview'
const BASE = 'd92d6aae'
const RUST = 'Rust: add $HOME/.cargo/bin (PowerShell $env:USERPROFILE\\.cargo\\bin) to PATH. Run tools/*.ps1 from the PowerShell tool with powershell -NoProfile -ExecutionPolicy Bypass -File. Build with CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0 and your own CARGO_TARGET_DIR.'

// [id, C# source (relative to src/PhotoReview.Imaging unless full), rust, model, review?]
const U = [
  ['W4-01a', 'ImageCacheKey.cs', 'crates/pr-imaging/src/cache_key.rs', 'sonnet', false],
  ['W4-01b', 'DecodeBox.cs, Decoding/DecodeRequest.cs, DecodedImageSize.cs, ImageInfo.cs, DecodedImage.cs, IDecodedImage.cs, PixelLease.cs, DecodeFailureSourceBytes.cs', 'crates/pr-imaging/src/decode_types.rs', 'sonnet', false],
  ['W4-01c', 'Decoding/DecoderBusyException.cs, DecoderMemoryAdmissionException.cs, MissingImageCodecException.cs, ICancellableImageDecoder.cs, IImageDecoder.cs, IImageDecoderFactory.cs, IPlatformImageCodec.cs', 'crates/pr-imaging/src/decoder_traits.rs', 'sonnet', false],
  ['W4-02a', 'Decoding/ExifOrientation.cs (and the transform logic of Imaging.Wpf/WpfExifOrientation.cs)', 'crates/pr-imaging/src/orientation.rs', 'sonnet', false],
  ['W4-02b', 'Decoding/EmbeddedThumbnailReader.cs', 'crates/pr-imaging/src/embedded_thumbnail.rs', 'sonnet', false],
  ['W4-03a', 'Metadata/TiffStructure.cs', 'crates/pr-imaging/src/tiff.rs', 'sonnet', false],
  ['W4-03b', 'Metadata/ExifParser.cs', 'crates/pr-imaging/src/exif_parser.rs', 'sonnet', false],
  ['W4-04a', 'Metadata/ExifSummary.cs, Metadata/ExifSummaryCodec.cs', 'crates/pr-imaging/src/exif_summary.rs', 'sonnet', false],
  ['W4-04b', 'Metadata/ExifQueryInterpreter.cs', 'crates/pr-imaging/src/exif_query.rs', 'sonnet', false],
  ['W4-05a', 'AdaptivePreviewPolicy.cs', 'crates/pr-imaging/src/adaptive_preview.rs', 'sonnet', false],
  ['W4-05b', 'PreloadWindow.cs, PreloadOrderService.cs, Preload/PreloadOptions.cs', 'crates/pr-imaging/src/preload_order.rs', 'sonnet', false],
  ['W4-06a', 'Preload/NavigationPace.cs', 'crates/pr-imaging/src/navigation_pace.rs', 'sonnet', false],
  ['W4-06b', 'Preload/PreloadBusyBackoff.cs, Preload/PreviewSizeSampler.cs', 'crates/pr-imaging/src/preload_backoff.rs', 'sonnet', false],
  ['W4-07a', 'Preload/RamBudgetPolicy.cs', 'crates/pr-imaging/src/ram_budget.rs', 'sonnet', false],
  ['W4-07b', 'Decoding/MemoryHeadroom.cs', 'crates/pr-imaging/src/memory_headroom.rs', 'sonnet', false],
  ['W4-16a', 'Pixels/PixelBuffer.cs, Pixels/PixelOps.cs', 'crates/pr-imaging/src/pixels.rs', 'sonnet', false],
  ['W4-16b', 'Pixels/NativePixelMemory.cs, Decoding/PixelBufferImageCodec.cs, Decoding/ReadOnlyMemoryStreamFactory.cs', 'crates/pr-imaging/src/pixel_memory.rs', 'sonnet', false],
  ['W4-08a', 'Caching/AtomicCacheFile.cs', 'crates/pr-imaging/src/cache/atomic_file.rs', 'sonnet', true],
  ['W4-08b', 'Caching/PreviewCacheFile.cs', 'crates/pr-imaging/src/cache/preview_file.rs', 'sonnet', true],
  ['W4-09', 'Caching/DiskCacheStore.cs (INV-2)', 'crates/pr-imaging/src/cache/disk_store.rs', 'opus', true],
  ['W4-10a', 'Caching/SourceBytesCache.cs, Caching/SourceBytesCachePolicy.cs (INV-2/INV-8)', 'crates/pr-imaging/src/cache/source_bytes.rs', 'opus', true],
  ['W4-10b', 'Caching/ThumbnailCache.cs', 'crates/pr-imaging/src/cache/thumbnails.rs', 'opus', true],
  ['W4-12a', 'Decoding/ImageDecoderFactory.cs (INV-12)', 'crates/pr-imaging/src/factory.rs', 'sonnet', true],
  ['W4-12b', 'Decoding/FallbackImageDecoder.cs', 'crates/pr-imaging/src/fallback.rs', 'sonnet', true],
  ['W4-12c', 'Decoding/WebpHeicRoutingDecoder.cs', 'crates/pr-imaging/src/webp_heic.rs', 'sonnet', true],
  ['W4-11a', 'Decoding/Wic/WicInterop.cs (COM unsafe)', 'crates/pr-platform-win/src/wic/interop.rs', 'opus', true],
  ['W4-11b', 'Decoding/Wic/WicCodecAvailability.cs', 'crates/pr-platform-win/src/wic/availability.rs', 'sonnet', true],
  ['W4-11c', 'Decoding/Wic/WicDirectDecoder.cs', 'crates/pr-platform-win/src/wic/decoder.rs', 'opus', true],
  ['W4-11d', 'Decoding/Wic/WicExifReader.cs', 'crates/pr-platform-win/src/wic/exif_reader.rs', 'sonnet', true],
  ['W4-11e', 'Pixels/WicPixelScaler.cs, Caching/WicCacheImageReader.cs, Caching/WicImageEncoder.cs', 'crates/pr-platform-win/src/wic/{scaler,cache_image}.rs', 'sonnet', true],
  ['W4-14', 'Caching/PreviewImageService.cs (INV-1/INV-2): split into 14a/14b/14c sub-units after reading it', 'crates/pr-imaging/src/service/', 'opus', true],
  ['W4-15', 'Preload/PreloadScheduler.cs, Preload/IPreloadTarget.cs: split into 15a/15b sub-units after reading it', 'crates/pr-imaging/src/preload/', 'opus', true],
]
const WRITE = { type: 'object', properties: { unit: { type: 'string' }, contract: { type: 'boolean' }, stub_files: { type: 'array', items: { type: 'string' } }, oracle_and_golden: { type: 'boolean' }, depends_on: { type: 'array', items: { type: 'string' } }, open_questions: { type: 'array', items: { type: 'string' } }, problems: { type: 'array', items: { type: 'string' } } }, required: ['unit', 'contract', 'problems'] }
const VER = { type: 'object', properties: { unit: { type: 'string' }, changes: { type: 'array', items: { type: 'string' } }, lead_decisions: { type: 'array', items: { type: 'string' } } }, required: ['unit', 'changes', 'lead_decisions'] }

phase('Setup')
await agent(`In repo ${REPO} (main checkout: fetch, fast-forward master): create worktree ${WT} on new branch wave4/contracts from origin/master. Delete nothing. Report HEAD.`, { label: 'setup', phase: 'Setup', model: 'haiku' })

const common = `Repo worktree: ${WT}. ${RUST} Read AGENTS.md, contracts/README.md (decisions D1-D28), docs/PROPOSED-DECISIONS.md, docs/WAVE-4-PLAN.md (your unit's row, crates and models), docs/MULTI-AGENT-PLAN.md sections 7, 10, 11, docs/PARITY.md, and ONE finished exemplar: contracts/W2-05.md with its stub, tools/oracle-cs/Commands/W2-05.cs and golden input/output. C# is the spec at baseline ${BASE}: read via git -C ${CS} show ${BASE}:src/PhotoReview.Imaging/<path> (Core files under src/PhotoReview.Core; WIC under Decoding/Wic). Never edit or check out the C# repo. Existing Rust to build on: crates/pr-core (abstractions, io, caching, time, text), crates/pr-imaging, crates/pr-platform-win, crates/pr-test-support. Write ONLY inside ${WT}/contracts/drafts-w4/<unit>/ . Per unit: contract.md (same sections as W2 contracts: header colour/porter/test porter/verifier, C# source with sizes, C# tests to port with file and test names (search tests/PhotoReview.Imaging.Tests and Core.Tests), PARITY rows, Owned files, Public API, semantics, traps incl. INV ids, Depends on, Done when, golden package, open questions), Rust stub file(s) with todo!() bodies and complete real signatures/data types that compile clippy-clean (put target paths exactly as in the plan), and where differentially testable oracle/<unit>.cs in the style of W2 oracles plus golden-inputs/*.json (do NOT run the oracle; the wiring step does). On-disk/cache formats stay byte-identical to C#. Unsafe only in pr-platform-win with SAFETY per block. Use fixtures under tests/fixtures (read-only). No Windows-only deps in pr-core. Do not commit.`

phase('Write')
const out = await pipeline(
  U,
  u => agent(`${common}\nUnit ${u[0]}. C# source: ${u[1]}. Rust target: ${u[2]}. Write the full package for this sub-unit only.`,
    { label: `write:${u[0]}`, phase: 'Write', model: u[3], schema: WRITE }),
  (w, u) => u[4]
    ? agent(`${common}\nIndependent review of contracts/drafts-w4/${u[0]}/ : re-read the C# source and tests; verify every claim, signature, missing behaviour, INV ids, test list completeness, dependencies on other W4/W3/W2 units, stub plausibility. Fix errors in place. Return changes and lead decisions.`,
        { label: `review:${u[0]}`, phase: 'Write', model: u[3] === 'opus' ? 'opus' : 'sonnet', schema: VER }).then(v => ({ unit: u[0], write: w, verify: v }))
    : { unit: u[0], write: w, verify: null }
)

phase('Wire')
const wire = await agent(`Lead wiring for Wave 4 contracts in ${WT} (branch wave4/contracts). ${RUST}
Do what the Wave 2 and Wave 3 wiring did (git show on the commits of PR #7 and #11): (1) move contracts/drafts-w4/<unit>/contract.md to contracts/<unit>.md; (2) copy stubs to their crate paths; add pub mod lines (alphabetical; create mod.rs for new dirs such as cache/, service/, preload/, wic/; add the Cargo.toml dependencies only if a stub needs them: windows crate features for pr-platform-win; keep pr-imaging free of Windows-only deps) ; (3) copy oracle/*.cs to tools/oracle-cs/Commands/ and add dispatch lines to tools/oracle-cs/Program.cs; the oracle project may need PhotoReview.Imaging.dll: extend tools/oracle.ps1 and OracleCs.csproj to build PhotoReview.Imaging from the C# archive like Core (never touch the C# repo); copy golden-inputs/*.json to tests/golden-inputs/; (4) run tools/oracle.ps1 -Commit ${BASE} one -Command per call; every golden deterministic (no cwd/clock/file-time/temp-path dependent output; run twice and compare); do not commit changes to pre-existing goldens; (5) cargo fmt --all; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test --workspace --locked clean; (6) add the Wave 4 roster (the plan's tiers and models) + lead decisions D29+ from the review reports to contracts/README.md, PARITY rows, task_on_progress.md with what was actually run; (7) rm -rf contracts/drafts-w4; commit ("feat: Wave 4 contracts, stubs and goldens" + "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"), push -u origin wave4/contracts, gh pr create to master (body: what was checked, what was NOT run; end with "🤖 Generated with [Claude Code](https://claude.com/claude-code)"). Report honestly any failing step.
Reports: ${JSON.stringify(out.filter(Boolean).map(o => ({ unit: o.unit, problems: o.write && o.write.problems, q: o.write && o.write.open_questions, dec: o.verify && o.verify.lead_decisions })))}`,
  { label: 'wire', phase: 'Wire', model: 'sonnet' })
return { n: out.filter(Boolean).length, wire }
