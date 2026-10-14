// Generates review-catalog.json (the golden INPUT) deterministically. Run: node gen-review-catalog.js > review-catalog.json
// Only inputs live here; the expected values are produced by the C# oracle (tools/oracle-cs command review-catalog).
const D = 'C:\\photos\\';
const P = (n) => D + n;
const e = (n, extra = {}) => ({ path: P(n), ...extra });
const reset = (names, mode = 'Separate', sidecars = null) => ({
  op: 'reset',
  entries: names.map((n) => (typeof n === 'string' ? e(n) : n)),
  mode,
  ...(sidecars ? { sidecars: sidecars.map(P) } : {}),
});
const setCurrent = (index) => ({ op: 'setCurrent', index });
const remove = (n) => ({ op: 'remove', path: P(n) });
const restore = (n, index) => ({ op: 'restore', path: P(n), index });
const grp = (jpeg, raw, xmp) => ({ jpeg: P(jpeg), raw: P(raw), ...(xmp ? { xmp: P(xmp) } : {}) });
const restoreMembers = (names, index, group = null, rawEnabled = true) => ({
  op: 'restoreMembers',
  paths: names.map((n) => (n.startsWith('C:') ? n : P(n))),
  index,
  ...(group ? { group } : {}),
  ...(rawEnabled === false ? { rawEnabled: false } : {}),
});
const replaceOrder = (names) => ({ op: 'replaceOrder', paths: names.map((n) => P(n)) });
const removePaths = (names) => ({ op: 'removePaths', paths: names.map((n) => (n.startsWith('C:') ? n : P(n))) });
const updateMetadata = (n, length, ticks, width = undefined, height = undefined) => ({
  op: 'updateMetadata', path: P(n), length, ticks, ...(width !== undefined ? { width } : {}), ...(height !== undefined ? { height } : {}),
});
const STAMP = 639000000000000000;
const cases = [];
const add = (name, probes, ops) => cases.push({ name, probes: probes.map(P), ops });

// ---- plain catalogs ------------------------------------------------------------------------------------------
add('empty-catalog-boundaries', ['anything.jpg'], [
  setCurrent(-1), setCurrent(0), remove('anything.jpg'), removePaths(['anything.jpg']), replaceOrder([]),
  restoreMembers([], 0), updateMetadata('anything.jpg', 1, STAMP), restore('   ', 0),
]);
add('single-item-removal-empties-the-catalog', ['photo1.jpg'], [reset(['photo1.jpg']), remove('photo1.jpg'), remove('photo1.jpg')]);
add('remove-middle-then-last', ['img1.jpg', 'img2.jpg', 'img3.jpg'], [
  reset(['img1.jpg', 'img2.jpg', 'img3.jpg']), setCurrent(1), remove('img2.jpg'), remove('img3.jpg'),
]);
add('remove-entry-before-current-keeps-the-photo', ['img1.jpg', 'img2.jpg', 'img3.jpg', 'img4.jpg'], [
  reset(['img1.jpg', 'img2.jpg', 'img3.jpg', 'img4.jpg']), setCurrent(2), remove('img1.jpg'),
]);
add('remove-entry-after-current-keeps-the-photo', ['img1.jpg', 'img2.jpg', 'img3.jpg', 'img4.jpg'], [
  reset(['img1.jpg', 'img2.jpg', 'img3.jpg', 'img4.jpg']), setCurrent(1), remove('img4.jpg'),
]);
add('remove-current-first-and-last', ['a.jpg', 'b.jpg', 'c.jpg', 'd.jpg'], [
  reset(['a.jpg', 'b.jpg', 'c.jpg', 'd.jpg']), remove('a.jpg'), setCurrent(2), remove('d.jpg'), remove('c.jpg'),
]);
add('remove-unknown-and-case-variant', ['img2.jpg', 'IMG2.JPG'], [
  reset(['img1.jpg', 'img2.jpg', 'img3.jpg']), setCurrent(2), remove('nothing.jpg'), remove('IMG2.JPG'),
]);
add('restore-clamps-and-prevents-duplicates', ['img0.jpg', 'img1.jpg', 'img2.jpg', 'img3.jpg', 'img4.jpg'], [
  reset(['img1.jpg', 'img3.jpg']), restore('img1.jpg', 0), restore('IMG1.JPG', 0), restore('img0.jpg', -1),
  restore('img4.jpg', 999), restore('img2.jpg', 2), restore('   ', 0),
]);
add('restore-moves-current-only-when-inserted-at-or-before-it', ['a.jpg', 'b.jpg', 'c.jpg', 'x.jpg', 'y.jpg', 'z.jpg'], [
  reset(['a.jpg', 'b.jpg', 'c.jpg']), setCurrent(1), restore('x.jpg', 2), restore('y.jpg', 1), restore('z.jpg', 0),
]);
add('restore-into-empty-catalog-selects-it', ['a.jpg'], [reset([]), restore('a.jpg', 5)]);
add('set-current-bounds', ['a.jpg'], [reset(['a.jpg', 'b.jpg']), setCurrent(2), setCurrent(-1), setCurrent(1), setCurrent(0)]);
add('reset-twice-resets-position-and-bumps-version', ['a.jpg', 'x.jpg'], [
  reset(['a.jpg', 'b.jpg']), setCurrent(1), reset(['x.jpg']), reset([]),
]);
add('replace-order-rejects-duplicate-with-right-count', ['a.jpg', 'b.jpg', 'c.jpg'], [
  reset(['a.jpg', 'b.jpg', 'c.jpg']), setCurrent(1), replaceOrder(['a.jpg', 'a.jpg', 'c.jpg']),
]);
add('replace-order-rejects-case-variant-duplicate', ['a.jpg', 'b.jpg', 'c.jpg'], [
  reset(['a.jpg', 'b.jpg', 'c.jpg']), setCurrent(1), replaceOrder(['A.JPG', 'a.jpg', 'c.jpg']),
]);
add('replace-order-rejects-unknown-member', ['a.jpg', 'b.jpg', 'c.jpg'], [
  reset(['a.jpg', 'b.jpg', 'c.jpg']), setCurrent(1), replaceOrder(['a.jpg', 'b.jpg', 'x.jpg']),
]);
add('replace-order-rejects-wrong-count', ['a.jpg', 'b.jpg', 'c.jpg'], [
  reset(['a.jpg', 'b.jpg', 'c.jpg']), setCurrent(2), replaceOrder(['a.jpg', 'b.jpg']), replaceOrder(['a.jpg', 'b.jpg', 'c.jpg', 'd.jpg']),
]);
add('replace-order-keeps-current-by-path-and-accepts-case-variants', ['a.jpg', 'b.jpg', 'c.jpg', 'C.JPG'], [
  reset(['a.jpg', 'b.jpg', 'c.jpg']), setCurrent(2), replaceOrder(['c.jpg', 'a.jpg', 'b.jpg']), replaceOrder(['B.JPG', 'C.jpg', 'A.jpg']),
]);
add('replace-order-with-case-variant-duplicates-in-the-catalog-never-throws', ['a.jpg', 'A.JPG'], [
  reset(['a.jpg', 'A.JPG']), replaceOrder(['A.JPG', 'a.jpg']), replaceOrder(['a.jpg', 'A.JPG']),
]);
add('replace-order-on-empty-and-single', ['a.jpg'], [reset([]), replaceOrder([]), reset(['a.jpg']), replaceOrder(['a.jpg'])]);
add('case-variant-duplicate-entries-first-wins-in-index', ['a.jpg', 'A.JPG', 'b.jpg'], [
  reset(['a.jpg', 'A.JPG', 'b.jpg']), remove('A.JPG'), remove('a.jpg'),
]);
add('remove-with-case-variant-duplicates-rebuilds-instead-of-incremental', ['a.jpg', 'b.jpg', 'c.jpg', 'A.JPG'], [
  reset(['a.jpg', 'b.jpg', 'A.JPG', 'c.jpg']), setCurrent(3), remove('b.jpg'), remove('a.jpg'),
]);
add('reset-skips-nothing-valid-and-keeps-metadata', ['a.jpg', 'b.jpg'], [
  reset([e('a.jpg', { length: 5, width: 10 }), e('b.jpg')]),
  updateMetadata('b.jpg', 7, STAMP, 100, 50), updateMetadata('a.jpg', 5, STAMP), updateMetadata('A.JPG', 6, STAMP + 1, 1, 2), updateMetadata('zzz.jpg', 1, STAMP),
]);

// ---- index: incremental removals ---------------------------------------------------------------------------------
{
  const names = Array.from({ length: 200 }, (_, i) => `p${String(i).padStart(3, '0')}.jpg`);
  const probes = ['p000.jpg', 'p001.jpg', 'p010.jpg', 'p063.jpg', 'p064.jpg', 'p065.jpg', 'p066.jpg', 'p100.jpg', 'p150.jpg', 'p199.jpg', 'P199.JPG'];
  const ops = [reset(names), setCurrent(100)];
  for (let i = 0; i < 70; i++) ops.push(remove(names[(i * 3) % 200 === 100 ? 101 : (i * 3) % 200]));
  ops.push(restore('p000.jpg', 0), remove('p199.jpg'), replaceOrder([]));
  add('index-stays-correct-through-70-removals-past-the-rebuild-threshold', probes, ops);
}
{
  const names = Array.from({ length: 30 }, (_, i) => `q${String(i).padStart(2, '0')}.jpg`);
  const probes = ['q00.jpg', 'q05.jpg', 'q10.jpg', 'q11.jpg', 'q12.jpg', 'q20.jpg', 'q29.jpg'];
  add('burst-of-removals-then-restores-and-lookups', probes, [
    reset(names), setCurrent(11), remove('q10.jpg'), remove('q12.jpg'), remove('q11.jpg'), remove('q00.jpg'), restore('q10.jpg', 10),
    remove('q29.jpg'), restore('q00.jpg', 0), remove('q05.jpg'), remove('q20.jpg'), restore('q12.jpg', 99),
  ]);
}

// ---- pairing (RawPairMode) -------------------------------------------------------------------------------------
add('prefer-jpeg-pair-found-through-either-member', ['a.jpg', 'a.cr2', 'A.CR2', 'b.jpg', 'a.xmp'], [
  reset(['a.jpg', 'a.cr2', 'b.jpg'], 'PreferJpeg', ['a.xmp']), setCurrent(1),
]);
add('prefer-raw-uses-the-raw-entry-and-collapses-member-orders', ['a.jpg', 'a.cr2', 'b.jpg'], [
  reset([e('a.jpg', { length: 10 }), e('a.cr2', { length: 100 }), e('b.jpg')], 'PreferRaw'),
  replaceOrder(['b.jpg', 'a.cr2', 'a.jpg']), replaceOrder(['a.cr2', 'b.jpg']), replaceOrder(['a.jpg', 'a.cr2', 'b.jpg']),
]);
add('separate-mode-keeps-pair-members-separate', ['a.jpg', 'a.cr2'], [reset(['a.jpg', 'a.cr2'], 'Separate', ['a.xmp'])]);
add('update-metadata-member-path-is-a-no-op-representative-stamps', ['p.jpg', 'p.cr2'], [
  reset(['p.jpg', 'p.cr2'], 'PreferJpeg'), updateMetadata('p.cr2', 99, STAMP, 6000, 4000), updateMetadata('P.JPG', 7, STAMP),
]);
add('member-order-collapse-rejects-unknown-and-repeated-members', ['a.jpg', 'a.cr2', 'b.jpg'], [
  reset(['a.jpg', 'a.cr2', 'b.jpg'], 'PreferJpeg'),
  replaceOrder(['a.jpg', 'a.cr2', 'zzz.jpg']), replaceOrder(['a.jpg', 'a.jpg', 'b.jpg']), replaceOrder(['b.jpg', 'a.cr2', 'a.jpg']),
  replaceOrder(['b.jpg', 'a.jpg']), replaceOrder(['A.CR2', 'B.JPG', 'A.JPG']),
]);
add('member-order-with-a-grouped-entry-listed-by-representative-only', ['a.jpg', 'a.cr2', 'b.jpg'], [
  reset(['a.jpg', 'a.cr2', 'b.jpg'], 'PreferJpeg'), replaceOrder(['b.jpg', 'a.jpg']), replaceOrder(['a.jpg', 'b.jpg']),
]);
add('group-entry-and-its-raw-member-listed-separately-share-the-member-path', ['A.jpg', 'A.cr2'], [
  reset([e('A.cr2'), e('A.jpg', { group: grp('A.jpg', 'A.cr2') })]), remove('A.cr2'), remove('A.cr2'),
]);
add('remove-by-member-path-removes-the-grouped-entry', ['a.jpg', 'a.cr2', 'b.jpg'], [
  reset(['a.jpg', 'a.cr2', 'b.jpg', 'c.jpg'], 'PreferJpeg'), setCurrent(2), remove('a.cr2'), remove('a.jpg'),
]);

// ---- restoreMembers ----------------------------------------------------------------------------------------------
const pairCat = (mode) => reset(['before.jpg', 'after.jpg'], mode);
const PR = ['before.jpg', 'after.jpg', 'p.jpg', 'p.cr2', 'p.xmp', 'raw.cr2', 'jpeg.jpg'];
add('restore-members-separate-mode-unique-plain-entries', PR, [pairCat('Separate'), restoreMembers(['raw.cr2', 'jpeg.jpg', 'raw.cr2'], 1)]);
add('restore-members-raw-off-restores-only-the-jpeg', PR, [pairCat('PreferJpeg'), restoreMembers(['p.jpg', 'p.cr2'], 1, grp('p.jpg', 'p.cr2'), false)]);
add('restore-members-webp-heic-restored-whatever-the-raw-switch', ['before.jpg', 'web.webp', 'IMG_1.HEIC', 'x.heif', 'y.avif'], [
  reset(['before.jpg']), restoreMembers(['web.webp', 'IMG_1.HEIC', 'x.heif', 'y.avif'], 1, null, false),
]);
add('restore-members-xmp-and-other-extensions-are-never-entries', PR, [
  pairCat('Separate'), restoreMembers(['p.xmp', 'notes.txt', 'p.jpg'], 1), restoreMembers(['only.xmp'], 0),
]);
add('restore-members-plain-reset-forgets-the-previous-pair-mode', PR, [
  pairCat('PreferJpeg'), reset(['before.jpg', 'after.jpg']), restoreMembers(['p.jpg', 'p.cr2'], 1, grp('p.jpg', 'p.cr2')),
]);
add('restore-members-both-image-members-prefer-jpeg', PR, [pairCat('PreferJpeg'), restoreMembers(['p.jpg', 'p.cr2'], 1)]);
add('restore-members-both-image-members-prefer-raw', PR, [pairCat('PreferRaw'), restoreMembers(['p.jpg', 'p.cr2'], 1)]);
add('restore-members-sidecar-travels-in-the-derived-group', PR, [pairCat('PreferJpeg'), restoreMembers(['p.jpg', 'p.cr2', 'p.xmp'], 1)]);
add('restore-members-only-a-sidecar-restores-nothing', PR, [pairCat('PreferJpeg'), restoreMembers(['p.xmp'], 0)]);
add('restore-members-explicit-group-is-preserved-with-its-sidecar', PR, [
  pairCat('PreferRaw'), restoreMembers(['p.jpg', 'p.cr2'], 0, grp('p.jpg', 'p.cr2', 'p.cr2.xmp')),
]);
add('restore-members-one-member-of-a-pair-is-ungrouped', PR, [pairCat('PreferJpeg'), restoreMembers(['p.cr2'], 1)]);
add('restore-members-keeps-current-on-the-displayed-image', PR, [pairCat('PreferJpeg'), setCurrent(1), restoreMembers(['p.jpg', 'p.cr2'], 1)]);
add('restore-members-into-empty-catalog-makes-it-current', PR, [reset([], 'PreferJpeg'), restoreMembers(['p.jpg', 'p.cr2'], 0)]);
add('restore-members-index-clamping', PR, [pairCat('Separate'), restoreMembers(['jpeg.jpg'], -5), restoreMembers(['raw.cr2'], 99)]);
add('restore-members-already-listed-path-is-not-added-again', PR, [
  reset(['a.jpg', 'b.jpg']), restoreMembers(['a.jpg'], 1), restoreMembers(['A.JPG', 'c.jpg'], 1),
]);
add('restore-members-blank-and-case-variant-duplicate-paths', PR, [
  pairCat('Separate'), restoreMembers(['', '  ', 'jpeg.jpg', 'JPEG.JPG', 'raw.cr2'], 1),
]);
add('restore-members-two-derived-groups-and-a-single', ['x.jpg', 'x.nef', 'y.jpg', 'y.arw', 'z.jpg', 'x.xmp'], [
  reset([], 'PreferRaw'), restoreMembers(['x.jpg', 'z.jpg', 'x.nef', 'y.arw', 'y.jpg', 'x.xmp'], 0),
]);
add('restore-members-explicit-group-with-only-one-member-present', PR, [
  pairCat('PreferJpeg'), restoreMembers(['p.jpg'], 1, grp('p.jpg', 'p.cr2')),
]);
add('restore-members-explicit-group-plus-extra-paths', ['x.jpg', 'x.nef', 'y.jpg', 'y.arw', 'p.jpg', 'p.cr2'], [
  reset([], 'PreferJpeg'), restoreMembers(['p.jpg', 'p.cr2', 'x.jpg', 'x.nef', 'y.jpg'], 0, grp('p.jpg', 'p.cr2', 'p.xmp')),
]);
for (const [mode, first, later, label] of [
  ['PreferJpeg', 'p.jpg', 'p.cr2', 'jpeg-first'],
  ['PreferJpeg', 'p.cr2', 'p.jpg', 'raw-first-current-keeps-the-displayed-member'],
  ['PreferRaw', 'p.jpg', 'p.cr2', 'prefer-raw-jpeg-first'],
  ['PreferRaw', 'p.cr2', 'p.jpg', 'prefer-raw-raw-first'],
]) {
  add(`restore-members-reform-second-member-after-first-alone-${label}`, PR, [
    pairCat(mode), restoreMembers([first], 1, grp('p.jpg', 'p.cr2', 'p.xmp')), setCurrent(1), restoreMembers([later], 1, grp('p.jpg', 'p.cr2', 'p.xmp')),
  ]);
}
for (const [mode, first, later, label] of [
  ['PreferJpeg', 'p.cr2', 'p.jpg', 'prefer-jpeg'],
  ['PreferRaw', 'p.jpg', 'p.cr2', 'prefer-raw'],
]) {
  add(`restore-members-reform-of-a-non-current-entry-follows-the-pair-mode-${label}`, PR, [
    pairCat(mode), restoreMembers([first], 1, grp('p.jpg', 'p.cr2')), setCurrent(2), restoreMembers([later], 1, grp('p.jpg', 'p.cr2')),
  ]);
}
add('restore-members-reform-with-both-members-standalone-second-is-current', PR, [
  pairCat('PreferJpeg'), restoreMembers(['p.jpg'], 1, grp('p.jpg', 'p.cr2')), restore('p.cr2', 2), setCurrent(2),
  restoreMembers(['p.cr2'], 1, grp('p.jpg', 'p.cr2')),
]);
add('restore-members-reform-with-both-members-standalone-first-is-current', PR, [
  pairCat('PreferJpeg'), restoreMembers(['p.jpg'], 1, grp('p.jpg', 'p.cr2')), restore('p.cr2', 2), setCurrent(1),
  restoreMembers([], 1, grp('p.jpg', 'p.cr2')),
]);
add('restore-members-partner-still-missing-keeps-the-independent-entry', PR, [
  pairCat('PreferJpeg'), restoreMembers(['p.jpg'], 1, grp('p.jpg', 'p.cr2')), restoreMembers(['p.jpg'], 1, grp('p.jpg', 'p.cr2')),
]);
add('restore-members-reform-keeps-current-on-the-photo-after-the-group', PR, [
  pairCat('PreferJpeg'), restoreMembers(['p.jpg'], 1, grp('p.jpg', 'p.cr2')), setCurrent(2), restoreMembers(['p.cr2'], 1, grp('p.jpg', 'p.cr2')),
]);
add('restore-members-second-member-while-the-first-is-the-only-entry', ['a.jpg', 'a.cr2'], [
  reset(['a.jpg'], 'PreferJpeg'), restoreMembers(['a.cr2'], 1, grp('a.jpg', 'a.cr2')),
]);
add('restore-members-reform-rejected-when-the-listed-member-is-already-grouped', PR, [
  reset(['before.jpg', 'p.jpg', 'p.cr2'], 'PreferJpeg'), restoreMembers(['p.cr2'], 1, grp('p.jpg', 'p.cr2')),
]);
add('restore-members-reform-rejected-when-raw-is-off', PR, [
  pairCat('PreferJpeg'), restoreMembers(['p.jpg'], 1, grp('p.jpg', 'p.cr2')), restoreMembers(['p.cr2'], 1, grp('p.jpg', 'p.cr2'), false),
]);
for (const [start, label] of [[0, 'before-the-folded-entries'], [4, 'after-them'], [3, 'on-the-removed-duplicate'], [1, 'on-the-surviving-slot']]) {
  add(`restore-members-both-members-listed-separately-fold-and-keep-the-current-photo-${label}`, ['x.jpg', 'a.jpg', 'a.cr2', 'y.jpg', 'z.jpg'], [
    reset(['x.jpg', 'a.jpg', 'y.jpg', 'z.jpg'], 'PreferJpeg'), restoreMembers(['a.cr2'], 3), setCurrent(start), restoreMembers([], 0, grp('a.jpg', 'a.cr2')),
  ]);
}

// ---- removePaths -------------------------------------------------------------------------------------------------
const five = ['a.jpg', 'b.jpg', 'c.jpg', 'd.jpg', 'e.jpg'];
add('remove-paths-non-current-keeps-the-current-photo', five, [
  reset(five), setCurrent(2), removePaths(['D.JPG', 'a.jpg', 'ghost.jpg']),
]);
for (const [cur, bad] of [[1, ['b.jpg']], [1, ['b.jpg', 'c.jpg']], [4, ['e.jpg']], [3, ['d.jpg', 'e.jpg']], [0, ['a.jpg', 'c.jpg']], [2, ['a.jpg', 'b.jpg', 'c.jpg', 'd.jpg']]]) {
  add(`remove-paths-removed-current-advances-like-delete-cur${cur}-bad-${bad.join('+')}`, five, [reset(five), setCurrent(cur), removePaths(bad)]);
}
add('remove-paths-all-or-none', five, [reset(['a.jpg', 'b.jpg']), removePaths(['x.jpg']), removePaths(['a.jpg', 'b.jpg'])]);
add('remove-paths-blank-and-duplicate-inputs', five, [reset(five), setCurrent(3), removePaths(['', '  ', 'b.jpg', 'B.JPG', 'b.jpg'])]);
add('remove-paths-reports-in-catalog-order-not-input-order', five, [reset(five), removePaths(['e.jpg', 'c.jpg', 'a.jpg'])]);
add('remove-paths-on-an-empty-catalog', five, [reset([]), removePaths(['a.jpg'])]);
const grouped = (mode, ...extra) => reset(['p.jpg', 'p.cr2', ...extra], mode);
for (const [mode, bad, label] of [['PreferRaw', 'p.cr2', 'raw-representative'], ['PreferJpeg', 'p.jpg', 'jpeg-representative']]) {
  add(`remove-paths-unreadable-${label}-degrades-to-the-partner`, ['p.jpg', 'p.cr2', 'z.jpg'], [
    reset([e('p.jpg', { length: 1 }), e('p.cr2', { length: 2 }), 'z.jpg'], mode), setCurrent(1), removePaths([bad]),
  ]);
}
add('remove-paths-unreadable-non-representative-keeps-the-representative-and-drops-the-group', ['p.jpg', 'p.cr2'], [
  reset([e('p.jpg', { length: 1 }), e('p.cr2', { length: 2 })], 'PreferJpeg'), removePaths(['p.cr2']),
]);
add('remove-paths-current-capture-degrades-and-stays-current', ['p.jpg', 'p.cr2', 'z.jpg'], [
  grouped('PreferRaw', 'z.jpg'), removePaths(['p.cr2']),
]);
add('remove-paths-both-image-members-remove-the-entry-and-report-both', ['p.jpg', 'p.cr2', 'z.jpg'], [
  grouped('PreferJpeg', 'z.jpg'), removePaths(['p.cr2', 'p.jpg']),
]);
add('remove-paths-both-members-case-variants-and-representative-first', ['p.jpg', 'p.cr2', 'z.jpg'], [
  grouped('PreferRaw', 'z.jpg'), removePaths(['P.JPG', 'P.CR2']),
]);
add('remove-paths-unrelated-entry-leaves-an-untouched-group-intact', ['a.jpg', 'a.cr2', 'b.jpg', 'c.jpg'], [
  reset(['a.jpg', 'a.cr2', 'b.jpg', 'c.jpg'], 'PreferJpeg'), removePaths(['c.jpg']),
]);
add('remove-paths-sidecar-path-removes-nothing', ['a.jpg', 'a.cr2', 'b.jpg'], [
  reset(['a.jpg', 'a.cr2', 'b.jpg'], 'PreferJpeg', ['a.xmp']), removePaths(['a.xmp']),
]);
add('remove-paths-degraded-current-is-kept-with-the-group-removed-before-it', ['x.jpg', 'p.jpg', 'p.cr2', 'z.jpg'], [
  reset(['x.jpg', 'p.jpg', 'p.cr2', 'z.jpg'], 'PreferJpeg'), setCurrent(2), removePaths(['x.jpg', 'p.jpg']),
]);
add('remove-paths-group-before-current-removed-current-moves', ['x.jpg', 'p.jpg', 'p.cr2', 'z.jpg', 'w.jpg'], [
  reset(['x.jpg', 'p.jpg', 'p.cr2', 'z.jpg', 'w.jpg'], 'PreferJpeg'), setCurrent(3), removePaths(['p.jpg', 'p.cr2', 'z.jpg']),
]);

// ---- randomized traces (model check against the C# oracle) -------------------------------------------------------
function rng(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const spell = (r, n) => (r() < 0.25 ? n.toUpperCase().replace('C:\\PHOTOS\\', 'C:\\photos\\') : n);
function randomTrace(seed, mode, withGroups, steps) {
  const r = rng(seed);
  const pick = (a) => a[Math.floor(r() * a.length)];
  const stems = ['a', 'b', 'c', 'd', 'e', 'f'];
  const jpegs = stems.map((s) => `${s}.jpg`);
  const raws = stems.slice(0, 4).map((s) => `${s}.cr2`);
  const universe = withGroups ? [...jpegs, ...raws] : jpegs;
  const probes = [...universe, 'zz.jpg', 'p.xmp'];
  const shuffle = (a) => a.map((x) => [r(), x]).sort((x, y) => x[0] - y[0]).map((x) => x[1]);
  const ops = [reset(shuffle(universe).slice(0, 5 + Math.floor(r() * 4)), mode, withGroups ? ['a.xmp', 'c.cr2.xmp'] : null)];
  for (let i = 0; i < steps; i++) {
    const k = r();
    if (k < 0.12) ops.push(setCurrent(Math.floor(r() * 8) - 1));
    else if (k < 0.3) ops.push(remove(spell(r, pick(universe))));
    else if (k < 0.42) ops.push(restore(pick(universe), Math.floor(r() * 10) - 2));
    else if (k < 0.62) {
      const n = 1 + Math.floor(r() * 3);
      const names = Array.from({ length: n }, () => pick(universe));
      if (r() < 0.2) names.push('a.xmp');
      const g = withGroups && r() < 0.4 ? pick([grp('a.jpg', 'a.cr2'), grp('b.jpg', 'b.cr2', 'b.xmp'), grp('c.jpg', 'c.cr2')]) : null;
      ops.push(restoreMembers(names, Math.floor(r() * 8) - 1, g, r() < 0.85));
    } else if (k < 0.78) ops.push(removePaths(Array.from({ length: 1 + Math.floor(r() * 3) }, () => spell(r, pick(universe)))));
    else if (k < 0.92) ops.push(replaceOrder(shuffle(universe).slice(0, Math.floor(r() * (universe.length + 1))).map((n) => spell(r, n))));
    else if (k < 0.97) ops.push(updateMetadata(pick(universe), Math.floor(r() * 100), STAMP + i, r() < 0.5 ? 100 : undefined));
    else ops.push(reset(shuffle(universe).slice(0, Math.floor(r() * 7)), mode, withGroups ? ['b.xmp'] : null));
  }
  add(`random-trace-seed${seed}-${mode}-${withGroups ? 'groups' : 'plain'}`, probes, ops);
}
for (const seed of [1, 2, 3, 4]) randomTrace(seed, 'Separate', false, 120);
for (const seed of [11, 12, 13]) randomTrace(seed, 'PreferJpeg', true, 120);
for (const seed of [21, 22, 23]) randomTrace(seed, 'PreferRaw', true, 120);
for (const seed of [31, 32]) randomTrace(seed, 'Separate', true, 100);

process.stdout.write(JSON.stringify({ cases }, null, 1) + '\n');
