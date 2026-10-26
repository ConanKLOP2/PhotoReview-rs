# Native input fixture history

Moved verbatim from the native fixture branch on 2026-10-26. History only; current state is in [task_on_progress.md](../../task_on_progress.md).

## Native fixture event-driven Background barrier, machine A, 2026-10-26 13:53

- Final6sourcee1f250c fmt12/workspace Clippy/workspace9862passed/0failed/1331ignored across564resultlines PASS, exit0. Combined regression100pass/1fail: clear_gpu_cache_after_dispose_is_a_no_op timed out at the Background sentinel; frame18/shell32/input15/menu22 allpassed, viewing13pass/1fail. No unchanged retry-to-green.
- The old wait_until continuously posts Normal-priority probes while waiting for a Background job, which is eligible only without higher-priority work/messages. Replaced that test's poll/AtomicBool with a bounded channel sent by the same real Background callback. The callback ordering and final gpu_cache_clear_count==0 assertion are preserved; no sleep, manual dispatcher drain or ignored failure.
- Updated combined regression101/101functions PASS: frame18, viewing14, shell32, input15, menus22, exit0. Includes actual FirstImagePresented, started/stopped glide, canonical dropped-file identity and all four popup paint rows. This run's60latency samples p95: handler0.10ms/layout0.11ms/frame1.04ms; C# handler/layout bounds PASS, WARP frame timing advisory. Resource-less executable icon branch stays unexercised.
- Final7fmt/Clippy/workspace gates follow before opening the fixture PR. No full production startup/hardware GPU/corpus/full mutation claim. Native/composition glue excluded from full cargo-mutants by policy. R1 remains pending merged fixture and final-head gates.

## Native input integrates merged lifetime repair, machine A, 2026-10-26 13:31

- Root PR260 merged8b93fbf; fetched full tree matches reviewed967c79f and FIX-shell-handler-lifetime claim released. Root source96971f3 fmt12/workspace Clippy/workspace9862passed/0failed/1331ignored (564resultlines) PASS, runner exit0; all50shell/frame functions PASS separately, executable icon resource branch unexercised.
- Integrated that merged root into native input fixture after its own final5gate and independent baseline diagnosis. Retain both task histories and all input/host/frame PARITY evidence. Final merged gate will run fmt, Clippy, workspace and101functions across shell-window/frame/viewing/input/menu serially; no failing assertion is skipped.
- App monitor tools remain unavailable. Computer-use inventory identified the desktop app as ChatGPT; skill guidance prohibits automating that UI, so no UI action or app auto-fix claim. GitHub squashauto was enabled on PR260 and the merge was verified through Git. Native fixture PR follows only after final combined verdict. R1 policy claim remains active and waits for these merged dependencies before its final PR gate.

## Native input final gate and independent lifetime defect, machine A, 2026-10-26 13:16

- Resumed after live quota reset0%5h/62%week. Runner72866 completed exit0 on21ff633: fmt12/Clippy and9862passed/0failed/1329ignored across564resultlines. Integrated tooling/docs master079a28d at74109b6; owned Rust source unchanged. Final5fmt12/workspace Clippy/workspace9862/0/1329 PASS, exit0.
- Broader real-tree regression on74109b6: frame17pass/1fail, viewing14/14PASS, input15/15PASS with actual FirstImagePresented/60latency samples, menus22/22PASS with all four popup capture rows. The frame lifetime case also failed individually on unchanged master079a28d, exit101. No unchanged retry-to-green; the test remains asserted.
- Independent FIX-shell-handler-lifetime A from079a28d in idle E9 root repairs window-handler-window Rc ownership after WM_NCDESTROY. Two new native regressions red before; root fix passes all50shell/frame functions afterward (icon resource branch still skipped without RT_GROUP_ICON). Root source36ad6de/docs96971f3, bounded final runner59109 active. Merge that root PR first, then integrate here and rerun final head gates before this fixture PR. All work committed/pushed; no full native startup/corpus/mutation claim.

## Native input final frame and latency parity, machine A, 2026-10-26 09:46

- E9 PR256 merged82b8418; fetched tree matches9ba59ab and W9-E9 claim released. Integrated E9 master at891da96. Prior native fixture source3a32e04 completed all12fmt/workspace Clippy and9851passed/0failed/1328ignored across564resultlines, exit0; this is pre-E9/pre-frame-strengthening evidence.
- Strengthened startup wait to the actual FirstImagePresented property. It exposed FakeHub.current_image always None: first test timed out, owned cargo18900/tree stopped after that failure (incomplete failed run). FakeHub now retains the image before broadcasting, allowing the real compositor to compare assigned/drawn pixel identity. No production fallback or relaxed draw criterion.
- Final selected native matrix15/15PASS in17.29s, exit0. Latency now uses LayoutUpdated/Render callbacks plus bounded channels, all60samples: handler p95=2.50ms, layout p95=1.12ms, frame p95=5.93ms; C# handler<50ms/layout<100ms assertions PASS. Frame timing remains hidden WARP/offscreen advisory, no hardware GPU claim. Started-glide/after-navigation-stop and canonical dropped-file identity assertions remain.
- Final integrated fmt/Clippy/workspace gates follow this source before PR. No full production startup/corpus/full mutation claim; fake hub/manual workers still bound the evidence. R1 runner17764 on8161e88 remains to collect, then integrate E9 master before its PR. Cleanup retained under earlier automatic approval rejection.

## Native input fixture repair, machine A, 2026-10-26

- Claimed FIX-native-input-fixture A after root PR255 merged aa119f4. Separate master-based codex/fix-native-input-fixture reuses idle root/target-w9-e3. No edits to machineB startup/composition. Baseline comparison confirmed all five prior failures on unchanged0fee72c, each exit101; the repaired borrow matrix on9b81ac3 was10pass/5fail.
- Native input harness now encodes real PNG/JPEG under owned TempRoot with the existing WIC encoder, decodes through WicDirectDecoder/PixelBufferImageCodec, attaches a WARP surface, forwards image events through the fake hub and waits for first present/nonzero settled geometry. Optional MainEnv initial-view callback delegates to the existing PresentationBridge with live settings/viewport; other fixtures keep their prior default. Relevant wheel/keep/glide dimensions match the pinned C# cases. Drop checks canonical file identity rather than a path containing parent segments or an8.3alias.
- Restored C# StopKinetic bool through the shell adapter; native glide test proves impulses start a glide and then asserts navigation already stopped it. Prior Rust test discarded this verdict. Final strengthened native input matrix15/15PASS serially in21.77s, after the initial repaired fixture15/15PASS in22.78s. All use owned files, no Recycle Bin. Package and workspace all-target Clippy PASS; all12package fmt PASS after fmt-all Windows206. Final workspace test runner39900 remains active on source3a32e04, TEMP/native-input-real-final-test-*. No production startup, hardware GPU/performance or corpus claim. Menu golden/mutation debts stay open. E9 PR256 opened with confirmed squashauto after final9862/0/1329 and75focused functions passed; pending merge. R1 pre-root9903/0/1328 gate completed, then integrated aa119f4 at8161e88; integrated runner17764 remains active. Collect those exact runners before source edits or fresh gates.
---

End of preserved sections.
