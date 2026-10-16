export const meta = {
  name: 'rtl-r6',
  description: 'Round 6: the probe kit into committed ignored ratchet suites with a test per case; verify 935b077 and hunt; fix the remaining classes by mechanism; price the owner items; review, verify, bounded fix loop',
  phases: [
    { title: 'Lanes', detail: 'K (kit into suites) and V (verify + hunt) in parallel' },
    { title: 'Fix', detail: 'F: fix by class on top of K, with V findings; price H1, H2-3, H6' },
    { title: 'Review', detail: 'hunt + code in parallel, then confirm' },
    { title: 'Verify', detail: 'adversarial reproduction of each finding' },
    { title: 'Loop', detail: 'at most 2 fix rounds, each with a fix check and a fresh hunt' },
  ],
}

const SP = '/tmp/claude-0/-home-user-tinker-pdf/14e5c5a7-848b-5101-b07a-814074112597/scratchpad'
const K = SP + '/k11r4'
const B56 = '/home/user/tinker-pdf/.claude/worktrees/wf_26917f32-492-8'
const R6 = SP + '/r6'
const JOURNALS = '/root/.claude/projects/-home-user-tinker-pdf/14e5c5a7-848b-5101-b07a-814074112597/subagents/workflows'

const CONTEXT = `
Repository: tinker-pdf. Your cwd is your own git worktree (refs are shared). First: git status --short (must be empty), git reset --hard develop, git log --oneline -1 (must be d1c32cd). develop's code is 935b077's (d1c32cd adds only the probe kit and docs on top).
AGENTS.md and CONTRIBUTING.md bind you (they are in your context through CLAUDE.md); read .claude/HANDOFF.md too, it is the state of the work. In particular: commits signed automatically as Ravindu Wijegunawardhana, exactly one trailer "Claude-Session: https://claude.ai/code/session_01UrZiwsx5M1t7AQ7cPJMxFk", never a Co-Authored-By or noreply line, no model names; do not push; never download a corpus; never skip, ignore or weaken a test to get green; never edit a determinism fingerprint or golden; fix by class not instance; a committed test for every case, failing first, with guard tests and counted injections, in both builds.
Environment: before cargo, export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0. 4 cores shared with one other agent; about 11 GB disk free, shared: check df -h / before a big build, never start one under 4 GB free, and delete your worktree's target/ (and any +1.99.0 target dir) when you finish. Probe runs take about 8 minutes per build in debug.
The probe kit is committed: tools/probes/rtl (README.md, families.tsv, probes/{pdf,shape,regressions,diag}, scripts/run.sh and comparators, baselines/{9297d2d,56dfd93,935b077}). Baseline trees for measuring something not in the committed baselines: ${K}/base is a 9297d2d worktree (CARGO_TARGET_DIR=${K}/base-target); ${B56} is a 56dfd93 worktree with a built target. Both are SHARED: never reset or commit there, copy in only your own files, remove exactly them afterwards, leave git status clean. For 935b077, develop's code is it.
The standard (three baselines): no book that conserved, read whole or was found by search at 9297d2d (before K-11), at 56dfd93, or at 935b077 (today) may fail at your tip, in either build, beyond the prices the owner accepted (docs/rulings.md ruling 14: the bracket tie-break's named price; the full price of the line-before exception, 12 October 2026). No glyph drawn over another under word-spacing or justification. No determinism hash moves. Every table you report: per family and build, conserving at 9297d2d -> 56dfd93 -> 935b077 -> tip; worse than each baseline (any metric, each named); gains; overprint. Check every run's exit status, done marker and row count before believing it.
Previous rounds' findings, for reference: the round-5 workflow journal ${JOURNALS}/wf_61f080ae-4b8/journal.jsonl and the round-4 review journal ${JOURNALS}/wf_26917f32-492/journal.jsonl (one JSON result per agent), the commit messages c7e9b89..935b077, docs/ROADMAP.md row CD-19 ("Left" (1)-(5)), and .claude/HANDOFF.md ("Still open", "Known issues in the probe kit").
`

const LANE = {
  type: 'object',
  properties: {
    branch: { type: 'string' },
    head: { type: 'string' },
    commits: { type: 'array', items: { type: 'string' } },
    table: { type: 'string' },
    tests_added: { type: 'string', description: 'every test added, with the case it covers, and that it failed first (or is a pin)' },
    injections: { type: 'string' },
    gates: { type: 'string' },
    open_items: { type: 'array', items: { type: 'string' } },
    owner_options: { type: 'string', description: 'for H1, H2-3, H6 (lane F): each candidate fix, its measured gains and costs per baseline and build, not committed' },
  },
  required: ['branch', 'head', 'commits', 'table', 'open_items'],
}
const FINDINGS = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    table: { type: 'string' },
    findings: { type: 'array', items: { type: 'object', properties: {
      id: { type: 'string' }, severity: { type: 'string', enum: ['blocking', 'should_fix', 'nit'] },
      kind: { type: 'string' }, mechanism: { type: 'string', description: 'the rule, state or code path that makes it fail; findings sharing one say so' },
      title: { type: 'string' }, detail: { type: 'string' },
      repro: { type: 'string', description: 'minimal book, exact command, text at 9297d2d, 56dfd93, 935b077 and the tip' },
      file: { type: 'string' }, line: { type: 'number' },
    }, required: ['id', 'severity', 'kind', 'mechanism', 'title', 'detail', 'repro'] } },
    artifacts: { type: 'string', description: 'probe sources and outputs a fixer can reuse (paths)' },
  },
  required: ['summary', 'findings', 'artifacts'],
}
const VERDICTS = {
  type: 'object',
  properties: { verdicts: { type: 'array', items: { type: 'object', properties: {
    id: { type: 'string' }, confirmed: { type: 'boolean' }, severity: { type: 'string', enum: ['blocking', 'should_fix', 'nit'] },
    evidence: { type: 'string' }, minimal_test: { type: 'string' },
  }, required: ['id', 'confirmed', 'evidence'] } } },
  required: ['verdicts'],
}
const SEVERITY = `Severity: blocking = a book worse than any of the three baselines beyond the owner-accepted prices; an overprint under word-spacing or justification; a panic, overflow or unchecked index on data from document bytes; super-linear blow-up; a moved hash; nondeterminism; a ratchet suite that can pass while a book got worse (a suite that does not test what it claims). should_fix = a false doc claim or commit-message number; a test that does not fail without its fix; a wrong injection count; a case found with no committed test; fmt/clippy. nit = wording.`

const verify = (rev, label, tip) => {
  const serious = ((rev && rev.findings) || []).filter(f => f.severity !== 'nit')
  if (!serious.length) return Promise.resolve([])
  return agent(`${CONTEXT}
${SEVERITY}
Your role: adversarially VERIFY another reviewer's findings on ${tip.branch} (${tip.head}): in your worktree git reset --hard ${tip.head}. Default to confirmed=false unless you reproduce it yourself. Scratch: ${R6}/verify-${label} (mkdir -p). For a regression, write a minimal styled_book test (crates/tinker-pdf/tests/epub_support) and show it holds at the baseline named (9297d2d in ${K}/base with its target; 56dfd93 in ${B56}; 935b077 = develop's code) and fails at the tip, in the build(s) named, and that it is not an owner-accepted price. Otherwise check against the code and run what can be run. You may change severity with a reason; put the minimal test in minimal_test. Findings (from ${label}; artifacts: ${rev.artifacts}):
${JSON.stringify(serious, null, 1)}`, { label: 'verify:' + label, phase: 'Verify', schema: VERDICTS, isolation: 'worktree' })
    .then(v => {
      const byId = new Map(serious.map(f => [f.id, f]))
      return ((v && v.verdicts) || []).filter(x => x.confirmed && x.severity !== 'nit').map(x => ({ from: label, ...(byId.get(x.id) || {}), verdict: x }))
    })
}

// ---------------- Lanes K and V ----------------
phase('Lanes')
const laneK = () => agent(`${CONTEXT}
You are LANE K: the probe kit becomes committed tests. Scratch: ${R6}/laneK (mkdir -p). RESUMING: an earlier run of this lane was stopped by a container restart in step 1. Its uncommitted work is saved as the signed WIP commit r6-laneK-wip (fc2e2d5, on develop d1c32cd): after the reset to develop, run git reset --hard r6-laneK-wip, read git show --stat HEAD, and read its notes and outputs in ${R6}/laneK. Check what of step 1 it finished (it changed the README, docs/verification.md, HANDOFF.md, the scripts and re-packed the 935b077 baselines), finish the rest, and fold the WIP into your step-1 commit (git reset --soft develop, then commit properly; no WIP subject may remain). Work in this order, one commit per step:
1. Fix the kit's known issues listed in .claude/HANDOFF.md ("Known issues in the probe kit", 1-6): run.sh's INT handling (stop the whole run and write no done marker on interrupt), RV4C_FILTER and every other probe filter unset by the runner (grep the probes for env::var), the false provenance line (README, the baseline note lines, and say in your commit that c4c456e's message was wrong: 7f1b3f8's crates/ tree equals 935b077's), table.py on partial runs (families not run are absent, not empty), VOV/TALLY/ZZ lines parsed and compared, and the nits (--help, progress file, empty lines/ dir, a pointer to the kit from docs/verification.md).
2. Turn every rows-kind family (families.tsv) into #[ignore]d RATCHET tests: one integration-test binary for tinker-pdf (crates/tinker-pdf/tests/rtl_probes.rs with one module per family under crates/tinker-pdf/tests/rtl_probes/) and one for tinker-pdf-shape (crates/tinker-pdf-shape/tests/rtl_lines.rs), so CI compiles two binaries, not seventy. Each test builds its family's books, prints a RAN banner with its row count, and FAILS if any book reads worse on any metric than the committed 935b077 baseline for that build (cfg(feature = "bundled-fonts") picks the build's baseline), naming each worse book; gains are printed, not failures. Baselines are read with first-party code only (ruling 13: no spawned program): re-pack them in a form this repository can decode itself (the engine's LZMA in tinker-pdf-archive, or Flate, or plain text if the size stays reasonable; keep the kit under about 25 MB) and update scripts/pack.py and the README to match. Timing families stay probes (no assertion on wall-clock). The scratch runner keeps working for exploration. Run every suite at the tip in both builds with -- --ignored --nocapture and grep the RAN banners; each must pass (the tip is 935b077's code). Prove each suite catches a regression: an injection (e.g. the CD-19 0 Tw rule put back, and one ruling-14 rule) must make named books fail; record counts.
3. A committed test for EVERY case found so far, even the small ones: collect every finding id from the two journals, the round-5 commit messages, ROADMAP CD-19's Left list and the 77 tests in probes/regressions (read them finding by finding; HANDOFF explains why 29/26 fail). For each case: if it is fixed at 935b077, a fast, non-ignored test that asserts the right reading (show it fails at the commit before its fix); if it is still open, a named pin of a known limit that asserts today's reading and says in its doc which ROADMAP row it belongs to and that it flips when the limit closes (the K-11 precedent in epub_conservation.rs). Group them by mechanism in a few test files (epub_conservation.rs, epub_fallback.rs, text_logical_order.rs, the shape crate's bidi tests); table-driven where shapes share a body. Both builds. Remove probes/regressions once every one of its tests is carried by a committed test, and say which carried which.
4. Docs: the kit README (how the suites and the scratch runner relate, how to refresh a baseline after an owner-accepted change), docs/verification.md (the suites, how to run them, the RAN banners), ROADMAP CD-19's Left list pointing at the pins.
Gates before each commit: cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo clippy -p tinker-pdf --all-targets --features bundled-fonts -- -D warnings; cargo +1.99.0 clippy for the crates you touched (own target dir, deleted after); cargo test -p tinker-pdf and with --features bundled-fonts and --no-default-features; cargo test -p tinker-pdf-shape; determinism and streaming_determinism in both builds; cargo run -p xtask -- check. Report the suites' RAN lines and counts, every test added with its case, and the injections.`, { label: 'lane:K', phase: 'Lanes', schema: LANE, isolation: 'worktree' })

const laneV = () => agent(`${CONTEXT}
${SEVERITY}
You are LANE V: the verification of 935b077's round-2 fix that was skipped, and a fresh hunt. Do not change engine code; your output is findings and probe sources. Scratch: ${R6}/laneV (mkdir -p). RESUMING: an earlier run of this lane was stopped by a container restart; its probe sources and partial outputs are in ${R6}/laneV. Reuse every run that finished (exit status, done marker and full row count), re-run the rest, and continue from there.
1. Review git diff b666461..935b077 (the round-2 fix, "read each right-to-left line back as a reader does, and keep a shared line's cell in pieces") for correctness, ruling 1, complexity (anything super-linear: build a worst case and time it at 56dfd93 and 935b077) and doc truth; check its three new tests fail at b666461; re-run its listed injections.
2. Hunt shapes no probe has tried (read tools/probes/rtl/families.tsv and the probe sources first and go elsewhere): other scripts the fonts cover, nested inline styles, lists, figures and captions, very narrow and very wide measures, mixed spacing units, pages breaking mid-paragraph, vertical writing if supported, plain synthetic PDFs (not EPUB) whose text right-to-left extraction reads, word-spacing and letter-spacing combinations around table cells, Latin glosses, numbers with every separator. Write new families in the kit's format (ROW lines readable by tools/probes/rtl/scripts), run them at 9297d2d, 56dfd93 and 935b077 in both builds, and report every book worse at 935b077 than at an earlier baseline, minimised, with the mechanism named; group findings that share a mechanism into one finding with all its shapes.
Return findings and the paths of your probe sources and outputs.`, { label: 'lane:V', phase: 'Lanes', schema: FINDINGS, isolation: 'worktree' })

const [kRes, vRes] = await parallel([() => laneK(), () => laneV()])
log(`lanes: K=${kRes ? kRes.head : 'FAILED'} V=${vRes ? (vRes.findings || []).length + ' findings' : 'FAILED'}`)
if (!kRes) return { stopped: 'lane K failed', vRes }
const vConfirmed = vRes ? await verify(vRes, 'laneV', { branch: 'develop', head: 'd1c32cd' }) : []
log(`lane V confirmed: ${vConfirmed.length}`)

// ---------------- Lane F ----------------
phase('Fix')
const laneF = await agent(`${CONTEXT}
${SEVERITY}
You are LANE F: fix by class. In your worktree: git reset --hard ${kRes.head} (lane K's work, also on branch r6-laneK, on develop; it holds the ratchet suites and the pins). RESUMING: earlier runs of this lane were stopped three times by container restarts, mid-fix. Its work is a chain of signed WIP commits on top of ${kRes.head}: 4af61f0, 9ec1b17, fc66e3c (guard tests for the fallback and the overprint guard), 693597f (V3 bundled guard, A5's injections), 68ac45b (A5: the run-space drawing taken only where it reads closer on the page) and 7fa1e40 = branch r6-laneF-wip (A6: two unexercised rules out, the f6_ems sweep, the cells guard, clippy; committed at 12:13, one minute before the restart, so nothing in the tree was lost). Run git reset --hard r6-laneF-wip, then read ${R6}/laneF/NOTES.md FIRST: it is your own record (variants, snap copies for the owner options, injection runs, the fold plan with msg1.txt, msg2.txt and tools/stages.py, and Next). Every background job died with the restart: the A5g full-suite run (both.sh A5g A5; the default build had finished 64 of 74 families, all 0 worse) and the timing job. MAKE LONG RUNS SURVIVE A RESTART: never start a whole suite run from zero when part of it finished; reuse every family output whose done marker and row count are complete (check against the baseline's row count), and re-run only the missing families; run each background job with the 2-hour timeout (7200000 ms), and write its progress to NOTES.md as it goes. continue from there, and fold the WIP into proper commits, one per mechanism (git reset --soft ${kRes.head}, then commit; no WIP subject may remain). Lane K's report: ${JSON.stringify({ tests_added: kRes.tests_added, open_items: kRes.open_items, table: kRes.table })}. Scratch: ${R6}/laneF (mkdir -p).
The open cases: ROADMAP CD-19's Left list and HANDOFF's "Still open" (H2-2 remainder, H2-7a-d and the smaller items need reader changes; H1, H2-3 and H6 are the owner's), plus lane V's confirmed findings:
${JSON.stringify(vConfirmed, null, 1)}
(lane V's probe sources: ${vRes ? vRes.artifacts : 'none'}).
1. Name each mechanism (the reader reads a line from the right; a stretch longer than an em is not resumed; the comma tie-break does not take a line opening with a number; and any mechanism lane V found). Group every case and finding by mechanism.
2. Sweep each mechanism's class with probes before changing code (vary script, direction, spacing, styling, structure, both builds, EPUB and plain PDFs); add the sweep families to the kit and to the ratchet suites, with baselines.
3. Fix the root of every mechanism that is not the owner's, once, for its whole class. Each case's pin flips into a fixed-case test (failing first at ${kRes.head}); add guard tests for neighbouring shapes that must not change; counted injections per rule; both builds; refresh the ratchet baselines only where a book got better, never where one got worse, and say which.
4. For H1, H2-3 and H6, which are the owner's: implement each candidate fix on a scratch branch only, measure it on every family against the three baselines in both builds, and report in owner_options what it gains and what it costs (books, shapes, line probes, BidiCharacterTest); do NOT commit them. A candidate that costs nothing anywhere may be committed, and then say so.
5. Docs in the same commits (ruling 14 if a rule changes, content-and-text.md, epub.md, ROADMAP CD-19 narrowed or closed). Gates as lane K's; every ratchet suite at the tip in both builds with its RAN banner. Signed commits, one per mechanism.
CHECKPOINTS (the container has restarted three times; make a restart cheap): at least every 30 minutes, and at every milestone (a mechanism named, a sweep run, a fix green, a baseline set written), commit a signed checkpoint in your worktree with git add -A and a subject starting WIP: (leave out throwaway probe copies and build output), and keep ${R6}/laneF/NOTES.md current with what is done, what each running job is, and what comes next. At the end fold every WIP into proper commits; no WIP subject may remain.`, { label: 'lane:F', phase: 'Fix', schema: LANE, isolation: 'worktree' })
if (!laneF) return { stopped: 'lane F failed', kRes, vRes, vConfirmed }

// ---------------- Review ----------------
const reviewPrompt = (role, tip, extra) => `${CONTEXT}
${SEVERITY}
The tip under review: branch ${tip.branch}, head ${tip.head}; in your worktree git reset --hard ${tip.head}. Its report: ${JSON.stringify({ commits: tip.commits, table: tip.table, tests_added: tip.tests_added, open_items: tip.open_items })}.
${role}
${extra || ''}
Return findings (mechanism named; findings that share a mechanism grouped) and the artifacts a fixer can reuse.`
const HUNT = (dir) => `Your role: HUNT for regressions and overprint in shapes no committed family covers (read tools/probes/rtl/families.tsv first). Scratch: ${dir} (mkdir -p). Write new families in the kit format, run them at 9297d2d, 56dfd93, 935b077 and the tip, both builds, and report every book worse at the tip than at any baseline, minimised, grouped by mechanism.`
const CODE = (dir) => `Your role: review the CODE, TESTS and DOCS of git diff d1c32cd..HEAD. Scratch: ${dir} (mkdir -p). Check: correctness against UAX #9, UAX #14 and ISO 32000-2 9.3.3; ruling 1; complexity; the ratchet suites really fail when a book reads worse (break one baseline row and one engine rule and watch them fail), read baselines with first-party code only, and print RAN banners; every case has a test, pins assert today's reading and name their row; each new test fails before its fix (check at least six); the injections reproduce; doc truth; fmt and clippy.`
const CONFIRM = (dir) => `Your role: CONFIRM. Scratch: ${dir} (mkdir -p). Re-run every ratchet suite and the kit's families at the tip in both builds, the line probes, every gate and determinism; re-measure every number in the commit messages and docs against the three baselines; check every case listed in the tests_added report exists and passes or pins as claimed.`

phase('Review')
const [hunt1, code1] = await parallel([
  () => agent(reviewPrompt(HUNT(R6 + '/hunt1'), laneF), { label: 'review:hunt', phase: 'Review', schema: FINDINGS, isolation: 'worktree' }),
  () => agent(reviewPrompt(CODE(R6 + '/code1'), laneF), { label: 'review:code', phase: 'Review', schema: FINDINGS, isolation: 'worktree' }),
])
const [vh, vc, confirm1] = await parallel([
  () => verify(hunt1, 'hunt1', laneF),
  () => verify(code1, 'code1', laneF),
  () => agent(reviewPrompt(CONFIRM(R6 + '/confirm1'), laneF), { label: 'review:confirm', phase: 'Review', schema: FINDINGS, isolation: 'worktree' }),
])
const vcf = await verify(confirm1, 'confirm1', laneF)
let confirmed = [...(vh || []), ...(vc || []), ...(vcf || [])]
const nits = [hunt1, code1, confirm1].filter(Boolean).flatMap(r => (r.findings || []).filter(f => f.severity === 'nit'))
log(`review: ${confirmed.length} confirmed, ${nits.length} nits`)

// ---------------- Loop ----------------
phase('Loop')
let tip = laneF
const rounds = []
const artifacts = [hunt1, code1, confirm1].filter(Boolean).map(r => r.artifacts).join('\n')
for (let round = 1; round <= 2 && confirmed.length; round++) {
  const fix = await agent(`${CONTEXT}
${SEVERITY}
You are the FIXER, round ${round}. In your worktree: git reset --hard ${tip.head} (branch ${tip.branch}). Scratch: ${R6}/fix${round} (mkdir -p). Fix every confirmed finding below by its mechanism, once per mechanism, each with tests per shape (failing first), guard tests and counted injections, plus the plainly correct nits; add the reviewers' probe families that found something to the kit and the ratchet suites. Do not reopen owner-accepted prices; anything that is the owner's goes to open_items with figures. Re-run every ratchet suite and family in both builds against the three baselines and against ${tip.head}; gates as before. Signed commits.
CHECKPOINTS (the container has restarted three times; make a restart cheap): at least every 30 minutes, and at every milestone, commit a signed checkpoint in your worktree with git add -A and a subject starting WIP: (leave out throwaway probe copies and build output), and keep ${R6}/fix${round}/NOTES.md current with what is done, what each running job is, and what comes next. At the end fold every WIP into proper commits; no WIP subject may remain.
Confirmed: ${JSON.stringify(confirmed, null, 1)}
Nits: ${JSON.stringify(round === 1 ? nits : [], null, 1)}
Reviewer artifacts: ${artifacts}`, { label: 'fix:' + round, phase: 'Loop', schema: LANE, isolation: 'worktree' })
  if (!fix) { rounds.push({ round, fix: null }); break }
  const [fv, hunt] = await parallel([
    () => agent(reviewPrompt(`Your role: verify the FIX of round ${round} only. Scratch: ${R6}/fixverify${round} (mkdir -p). Review git diff ${tip.head}..HEAD; check each finding is fixed and its tests fail at ${tip.head}; re-run every ratchet suite, the families and the line probes in both builds against the three baselines and ${tip.head}; gates and determinism. Findings: ${JSON.stringify(confirmed.map(c => ({ id: c.id, from: c.from, title: c.title })), null, 1)}`, fix), { label: 'fixverify:' + round, phase: 'Loop', schema: FINDINGS, isolation: 'worktree' }),
    () => agent(reviewPrompt(HUNT(R6 + '/hunt' + (round + 1)), fix, 'Go where the earlier hunters (their sources under ' + R6 + '/hunt* and ' + R6 + '/laneV) did not.'), { label: 'hunt:' + (round + 1), phase: 'Loop', schema: FINDINGS, isolation: 'worktree' }),
  ])
  const [a, b] = await parallel([() => verify(fv, 'fixverify' + round, fix), () => verify(hunt, 'hunt' + (round + 1), fix)])
  tip = fix
  confirmed = [...(a || []), ...(b || [])]
  rounds.push({ round, fix, fixVerify: fv, hunt, confirmed })
  log(`fix round ${round}: tip ${fix.head}; still confirmed ${confirmed.length}`)
}

return { kRes, vRes, vConfirmed, laneF, review: { hunt1, code1, confirm1 }, rounds, finalTip: { branch: tip.branch, head: tip.head }, stillOpen: confirmed, ownerOptions: laneF.owner_options, nits }
