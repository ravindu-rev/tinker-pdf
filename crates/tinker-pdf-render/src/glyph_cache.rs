//! The glyph coverage cache (`docs/design/performance.md`, P-06): one
//! render's glyph masks, kept under the key a mask is a function of.
//!
//! A source that keeps masks beyond one render — a document under a stated
//! budget (P-16) — holds them instead, asked by the same key with the font as
//! its id and the render's two rasterising settings beside it
//! (`GlyphSource::kept_mask`); this cache then holds none, and one budget
//! bounds the document's masks with everything else it keeps.
//!
//! A mask is the coverage of one glyph's outline under its quantised 2x2 and
//! its origin's phase (`glyph.rs`), flattened at the flatness in force
//! (10.7.2), so it is a function of five things — the font, the code, the
//! 2x2, the phase and the flattening tolerance — and those are its [`Key`].
//! The origin is not in it: the same glyph at the same phase is the same pixels
//! wherever it stands. **A hit is the mask a miss rasterises**: the renderer
//! keeps a mask only when its rasterisation ran to the end, so whether a
//! glyph hit moves no pixel.
//!
//! **Bounded by bytes, evicted by the sequence of requests alone.** Each
//! entry is charged its mask's pixels and [`ENTRY_BYTES`] beside them, against
//! [`MAX_GLYPH_CACHE_BYTES`]. Every request, hit or miss, takes the next
//! number of a counter, and an entry records the number of the request that
//! last asked for it; a mask that would not fit evicts the entries least
//! recently asked first, through an ordered index from that number to the
//! key, until it does. The `HashMap` that finds an entry by its key is never
//! iterated — its order is seeded per process — so which entries a full cache
//! evicts is the same on every run and every target.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use tinker_pdf_raster::fill::Mask;

/// The bytes one render's masks may hold.
///
/// A shape has up to 256 phases, a sixteenth of a pixel on each axis, so a
/// page asks for as many masks of one shape as the phases its glyphs of it
/// fall at — never more than it draws. A page of 3 000 glyphs at 300 dpi,
/// whose masks are some fifty pixels a side, keeps every one of them inside
/// this; a page that asks for more evicts, and a mask evicted is the same
/// pixels when it is rasterised again.
pub(crate) const MAX_GLYPH_CACHE_BYTES: u64 = 16 << 20;

/// The bytes an entry is charged beside its mask's pixels: its key, its
/// bookkeeping and the index entry beside it, so that a page of empty masks
/// is bounded too.
pub(crate) const ENTRY_BYTES: u64 = 64;

/// The font a mask was rasterised for, as a key holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum KeyFont {
    /// A font with an object of its own, `num << 16 | gen`: one font wherever
    /// a scope names it.
    Object(u64),
    /// A font written into a scope's own resource dictionary: the id its
    /// glyphs carry there, and the renderer's serial for the scope's turn.
    Scoped {
        /// The id the glyph carries.
        font_id: u64,
        /// The scope's serial: 0 for the page's, and the next number at each
        /// scope a form brings or a caller pushes.
        scope: u64,
    },
}

/// What a mask is a function of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    /// The font.
    pub(crate) font: KeyFont,
    /// The code the glyph was shown by.
    pub(crate) code: u32,
    /// The quantised em-to-device 2x2 (`glyph::Placement::matrix`).
    pub(crate) matrix: [i32; 4],
    /// The origin's phase on each axis (`glyph::Placement::phase`).
    pub(crate) phase: [u8; 2],
    /// The tolerance the outline is flattened at, in device pixels (10.7.2),
    /// as the bits of the `f64`: one glyph shown at two flatnesses is two
    /// masks.
    pub(crate) tolerance: u64,
    /// The bits of coverage the mask keeps, the text level
    /// (`docs/design/anti-aliasing.md`): a mask made at one level is never
    /// drawn at another.
    pub(crate) bits: u8,
    /// The rule it is whole or empty by at 0 bits, `Half` at any other.
    pub(crate) edge: tinker_pdf_raster::HardEdge,
}

/// One kept mask.
struct Entry {
    mask: Arc<Mask>,
    /// The number of the request that last asked for it.
    asked: u64,
    /// What it is charged against the bound.
    bytes: u64,
}

/// One render's masks.
pub(crate) struct GlyphCache {
    entries: HashMap<Key, Entry>,
    /// Each entry's last request, to its key: the order the cache evicts in,
    /// least recently asked first.
    order: BTreeMap<u64, Key>,
    /// The requests made so far, hits and misses alike.
    requests: u64,
    /// What the entries are charged, together.
    bytes: u64,
    /// What they may be charged.
    limit: u64,
}

impl GlyphCache {
    /// An empty cache under [`MAX_GLYPH_CACHE_BYTES`].
    pub(crate) fn new() -> Self {
        Self::with_limit(MAX_GLYPH_CACHE_BYTES)
    }

    /// An empty cache under `limit` bytes.
    pub(crate) fn with_limit(limit: u64) -> Self {
        GlyphCache {
            entries: HashMap::new(),
            order: BTreeMap::new(),
            requests: 0,
            bytes: 0,
            limit,
        }
    }

    /// What one entry of `mask` is charged.
    fn charge(mask: &Mask) -> u64 {
        (u64::from(mask.width) * u64::from(mask.height)).saturating_add(ENTRY_BYTES)
    }

    /// The mask kept under `key`, recorded as asked at this request; or, on a
    /// miss, the request's number, for [`GlyphCache::insert`] to keep the
    /// mask the miss rasterises under.
    pub(crate) fn get(&mut self, key: &Key) -> Result<Arc<Mask>, u64> {
        self.requests = self.requests.saturating_add(1);
        let request = self.requests;
        let Some(entry) = self.entries.get_mut(key) else {
            return Err(request);
        };
        self.order.remove(&entry.asked);
        entry.asked = request;
        self.order.insert(request, *key);
        Ok(Arc::clone(&entry.mask))
    }

    /// Keeps `mask` under `key`, asked at the request `asked` that missed it,
    /// evicting the entries least recently asked until it fits. A mask that
    /// could not fit an empty cache is not kept.
    pub(crate) fn insert(&mut self, key: Key, mask: Arc<Mask>, asked: u64) {
        let bytes = Self::charge(&mask);
        if bytes > self.limit {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.order.remove(&old.asked);
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        while self.bytes.saturating_add(bytes) > self.limit {
            let Some((_, evicted)) = self.order.pop_first() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&evicted) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
        self.order.insert(asked, key);
        self.entries.insert(key, Entry { mask, asked, bytes });
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// How many masks are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether a mask is kept under `key`, without asking for it.
    #[cfg(test)]
    pub(crate) fn holds(&self, key: &Key) -> bool {
        self.entries.contains_key(key)
    }

    /// What the kept masks are charged.
    #[cfg(test)]
    pub(crate) fn charged(&self) -> u64 {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        page_transform, Canvas, Color, Device, FontIdentity, GlyphSource, MaskKey, PixelFormat,
        Renderer,
    };
    use tinker_pdf_content::{GraphicsState, Matrix};
    use tinker_pdf_font::{Outline, Segment};

    /// A key for code `code` of font object `font` at phase `(x, y)`.
    fn key(font: u64, code: u32, x: u8, y: u8) -> Key {
        Key {
            font: KeyFont::Object(font),
            code,
            matrix: [3072, 0, 0, -3072],
            phase: [x, y],
            tolerance: 0.2f64.to_bits(),
            bits: 8,
            edge: tinker_pdf_raster::HardEdge::Half,
        }
    }

    /// A mask of `width` by `height`, every pixel `value`.
    fn mask(width: u32, height: u32, value: u8) -> Arc<Mask> {
        let mut mask = Mask::empty(0, 0, width, height);
        mask.data.iter_mut().for_each(|v| *v = value);
        Arc::new(mask)
    }

    /// Asks for `key`, keeping a fresh mask of `pixels` pixels on a miss, and
    /// says whether it hit.
    fn ask(cache: &mut GlyphCache, key: Key, pixels: u32) -> bool {
        match cache.get(&key) {
            Ok(_) => true,
            Err(asked) => {
                cache.insert(key, mask(pixels, 1, 255), asked);
                false
            }
        }
    }

    #[test]
    fn a_hit_hands_back_the_mask_the_miss_kept() {
        let mut cache = GlyphCache::new();
        let k = key(5 << 16, 65, 3, 0);
        let Err(asked) = cache.get(&k) else {
            panic!("an empty cache holds nothing");
        };
        let kept = mask(12, 14, 200);
        cache.insert(k, Arc::clone(&kept), asked);
        let hit = cache.get(&k).expect("kept");
        assert!(Arc::ptr_eq(&hit, &kept), "the very mask, not a copy of it");
        assert_eq!(cache.charged(), 12 * 14 + ENTRY_BYTES);
        // Another phase, another code and another font are other keys, and
        // so are another text level and another rule (RC-11.5): a mask made
        // at one is never drawn at another.
        for other in [
            key(5 << 16, 65, 4, 0),
            key(5 << 16, 66, 3, 0),
            key(6 << 16, 65, 3, 0),
            Key { bits: 4, ..k },
            Key {
                bits: 0,
                edge: tinker_pdf_raster::HardEdge::Centre,
                ..k
            },
            Key {
                bits: 0,
                edge: tinker_pdf_raster::HardEdge::AnyPart,
                ..k
            },
        ] {
            assert!(cache.get(&other).is_err(), "{other:?} is not {k:?}");
        }
    }

    /// The entries a run of requests leaves in a cache that holds three of
    /// them: whatever the hash order, the least recently asked go first.
    fn survivors() -> Vec<Key> {
        // Each mask is 100 pixels and 64 bytes, and the cache holds three.
        let mut cache = GlyphCache::with_limit(3 * (100 + ENTRY_BYTES));
        let a = key(1 << 16, 1, 0, 0);
        let b = key(1 << 16, 2, 0, 0);
        let c = key(1 << 16, 3, 0, 0);
        let d = key(1 << 16, 4, 0, 0);
        let e = key(2 << 16, 1, 15, 15);
        let mut hits = Vec::new();
        for k in [a, b, c, a, d, a, e, d, c] {
            hits.push(ask(&mut cache, k, 100));
        }
        assert_eq!(
            hits,
            [false, false, false, true, false, true, false, true, false],
            "a, b and c fill it; a hits; d evicts b, the least recently asked; \
             a hits; e evicts c, a having been asked since; d hits; c evicts a"
        );
        assert_eq!(cache.len(), 3, "never more than the bound holds");
        assert!(cache.charged() <= 3 * (100 + ENTRY_BYTES));
        [a, b, c, d, e]
            .into_iter()
            .filter(|k| cache.holds(k))
            .collect()
    }

    #[test]
    fn a_full_cache_evicts_the_same_entries_on_every_run() {
        let first = survivors();
        assert_eq!(
            first,
            [
                key(1 << 16, 3, 0, 0),
                key(1 << 16, 4, 0, 0),
                key(2 << 16, 1, 15, 15)
            ],
            "c, d and e: the three asked last"
        );
        // A hash map is seeded per process and per map; the order the cache
        // evicts in is not its order.
        for _ in 0..16 {
            assert_eq!(survivors(), first);
        }
    }

    #[test]
    fn a_mask_past_the_whole_bound_is_drawn_and_not_kept() {
        let mut cache = GlyphCache::with_limit(1000);
        let small = key(1 << 16, 1, 0, 0);
        assert!(!ask(&mut cache, small, 100));
        let large = key(1 << 16, 2, 0, 0);
        assert!(!ask(&mut cache, large, 1000), "1 000 pixels and 64 bytes");
        assert!(!cache.holds(&large), "past the bound, not kept");
        assert!(cache.holds(&small), "and nothing evicted for it");
        assert!(!ask(&mut cache, large, 1000), "a miss again, every time");
    }

    #[test]
    fn eviction_frees_exactly_what_a_new_mask_needs() {
        let mut cache = GlyphCache::with_limit(3 * (100 + ENTRY_BYTES));
        let keys: Vec<Key> = (0..3).map(|code| key(1 << 16, code, 0, 0)).collect();
        for &k in &keys {
            ask(&mut cache, k, 100);
        }
        // A mask twice the size evicts the two asked first and keeps the
        // third.
        let wide = key(1 << 16, 9, 0, 0);
        ask(&mut cache, wide, 200 + ENTRY_BYTES as u32);
        assert!(!cache.holds(&keys[0]) && !cache.holds(&keys[1]));
        assert!(cache.holds(&keys[2]) && cache.holds(&wide));
        assert_eq!(
            cache.charged(),
            (100 + ENTRY_BYTES) + (200 + 2 * ENTRY_BYTES)
        );
    }

    /// A glyph source of one glyph shape for every code, which names its
    /// fonts by `identity`, and whose form `Fm` brings a scope of its own
    /// drawing `form_shape` under the same font ids. Where it has a `store`
    /// it keeps the render's masks itself, as a document under a budget does.
    struct Source {
        shape: Outline,
        identity: Option<FontIdentity>,
        form: Option<Arc<Source>>,
        store: Option<Store>,
    }

    /// Masks a source keeps beyond one render: each kept under its key, the
    /// requests it was asked counted, and every key it was asked recorded.
    #[derive(Default)]
    struct Store {
        masks: std::sync::Mutex<HashMap<MaskKey, Arc<Mask>>>,
        asked: std::sync::Mutex<Vec<MaskKey>>,
    }

    impl GlyphSource for Source {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            Some(self.shape.clone())
        }

        fn font_identity(&self, _font_id: u64) -> Option<FontIdentity> {
            self.identity
        }

        fn form_scope(&self, name: &[u8]) -> Option<Arc<Self>> {
            (name == b"Fm").then(|| self.form.clone()).flatten()
        }

        fn kept_mask(&self, key: &MaskKey) -> Option<Result<Arc<Mask>, u64>> {
            let store = self.store.as_ref()?;
            let mut asked = store.asked.lock().expect("unpoisoned");
            asked.push(*key);
            let request = asked.len() as u64;
            let masks = store.masks.lock().expect("unpoisoned");
            Some(masks.get(key).cloned().ok_or(request))
        }

        fn keep_mask(&self, key: &MaskKey, mask: &Arc<Mask>, _asked: u64) {
            if let Some(store) = &self.store {
                store
                    .masks
                    .lock()
                    .expect("unpoisoned")
                    .insert(*key, Arc::clone(mask));
            }
        }
    }

    /// A box of one em, inset by `inset` em on every side.
    fn square(inset: f64) -> Outline {
        let (lo, hi) = (inset, 1.0 - inset);
        Outline {
            segments: vec![
                Segment::MoveTo { x: lo, y: lo },
                Segment::LineTo { x: hi, y: lo },
                Segment::LineTo { x: hi, y: hi },
                Segment::LineTo { x: lo, y: hi },
                Segment::Close,
            ],
        }
    }

    /// The glyph shown at the page's user `(x, y)`, font 7, code 65, 20 points.
    fn glyph(x: f64, y: f64) -> tinker_pdf_content::Glyph {
        tinker_pdf_content::Glyph {
            font_id: 7,
            code: 65,
            transform: Matrix {
                a: 20.0,
                b: 0.0,
                c: 0.0,
                d: 20.0,
                e: x,
                f: y,
            },
            text: "A".into(),
            baseline: None,
            advance: 1.0,
            size: 20.0,
            vertical: false,
            font_name: None,
            glyph_id: None,
            extent: None,
        }
    }

    /// Shows one glyph on the page at `(10, 10)` and one inside the form at
    /// `(60, 10)`, both font 7 code 65 at one phase, and returns the canvas
    /// and the counts.
    fn page_and_form(identity: Option<FontIdentity>) -> (Canvas, crate::RenderCounts) {
        let form = Arc::new(Source {
            shape: square(0.3),
            identity,
            form: None,
            store: None,
        });
        let page = Source {
            shape: square(0.0),
            identity,
            form: Some(form),
            store: None,
        };
        let canvas = Canvas::new(100, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), &page);
        let state = GraphicsState::default();
        renderer.show_glyph(&glyph(10.0, 10.0), &state);
        assert!(renderer.begin_form(1, b"Fm"));
        renderer.show_glyph(&glyph(60.0, 10.0), &state);
        renderer.end_form(1);
        let (canvas, _, counts) = renderer.finish_counted();
        (canvas, counts)
    }

    /// Black pixels in the columns `x0..x1`.
    fn black(canvas: &Canvas, x0: u32, x1: u32) -> usize {
        (0..40)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| canvas.pixel(x, y) == Some(Color::BLACK))
            .count()
    }

    #[test]
    fn two_scopes_naming_two_fonts_alike_never_share_a_mask() {
        // Both fonts are written into their scopes' dictionaries: one font
        // id, two fonts. The form's glyph, at the same phase as the page's,
        // must be the form's own square, inset, and not the page's mask.
        let (canvas, counts) = page_and_form(Some(FontIdentity::Scoped));
        let page = black(&canvas, 0, 50);
        let form = black(&canvas, 50, 100);
        assert!(page > 300, "the page's glyph fills its em: {page}");
        assert!(
            form > 0 && form < page / 2,
            "the form's is inset: {form} against {page}"
        );
        assert_eq!(
            counts.glyph_masks_rasterised, 2,
            "a mask for each scope's font"
        );
        assert_eq!(counts.glyph_mask_hits, 0, "and neither found the other's");

        // With no identity at all the cache is not asked: the same pixels,
        // every mask rasterised.
        let (plain, counts) = page_and_form(None);
        assert!(plain.data == canvas.data, "the same pixels either way");
        assert_eq!(
            (counts.glyph_masks_rasterised, counts.glyph_mask_hits),
            (2, 0)
        );
    }

    #[test]
    fn one_font_object_is_one_font_in_every_scope() {
        // The same object named in both scopes: the form's glyph, at the
        // same phase, is the page's mask, and it is drawn as the page's.
        // (Its source would have drawn an inset square, which an object's
        // identity says it does not have: one object, one outline.)
        let (canvas, counts) = page_and_form(Some(FontIdentity::Object(9 << 16)));
        assert_eq!(counts.glyph_masks_rasterised, 1);
        assert_eq!(counts.glyph_mask_hits, 1);
        assert_eq!(black(&canvas, 0, 50), black(&canvas, 50, 100));
    }

    #[test]
    fn a_pushed_scope_takes_a_serial_of_its_own() {
        let pushed = Arc::new(Source {
            shape: square(0.3),
            identity: Some(FontIdentity::Scoped),
            form: None,
            store: None,
        });
        let page = Source {
            shape: square(0.0),
            identity: Some(FontIdentity::Scoped),
            form: None,
            store: None,
        };
        let canvas = Canvas::new(100, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), &page);
        let state = GraphicsState::default();
        renderer.show_glyph(&glyph(10.0, 10.0), &state);
        renderer.push_resources(pushed);
        renderer.show_glyph(&glyph(60.0, 10.0), &state);
        renderer.pop_resources();
        // Back in the page's scope, the page's glyph again is a hit.
        renderer.show_glyph(&glyph(30.0, 10.0), &state);
        let (canvas, _, counts) = renderer.finish_counted();
        assert_eq!(counts.glyph_masks_rasterised, 2);
        assert_eq!(
            counts.glyph_mask_hits, 1,
            "the page's scope's mask, found again"
        );
        assert!(black(&canvas, 50, 100) < black(&canvas, 0, 25));
    }

    /// Three glyphs of font 7, code 65: at `(10, 10)` and `(30, 10)`, one
    /// phase, and at `(50.5, 10)`, half a pixel over; rendered through
    /// `source`, anti-aliased as `antialias` says.
    fn three_glyphs(source: &Source, antialias: bool) -> (Canvas, crate::RenderCounts) {
        let canvas = Canvas::new(100, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer =
            Renderer::new(canvas, page_transform(40.0, 1.0), source).with_antialias(antialias);
        let state = GraphicsState::default();
        for x in [10.0, 30.0, 50.5] {
            renderer.show_glyph(&glyph(x, 10.0), &state);
        }
        assert_eq!(renderer.glyph_cache.len(), 0, "the source holds the masks");
        let (canvas, _, counts) = renderer.finish_counted();
        (canvas, counts)
    }

    /// **A source that keeps masks beyond the render holds them, and the
    /// render none**: three glyphs, two at one phase, ask the source three
    /// times and rasterise twice, and draw what a render that keeps its own
    /// draws; a second render through the same source rasterises nothing and
    /// draws the same pixels; and one that does not anti-alias is asked by
    /// keys of its own, so it never draws another setting's mask.
    #[test]
    fn a_source_that_keeps_masks_holds_them_across_renders() {
        let source = Source {
            shape: square(0.1),
            identity: Some(FontIdentity::Object(9 << 16)),
            form: None,
            store: Some(Store::default()),
        };
        let own = Source {
            shape: square(0.1),
            identity: Some(FontIdentity::Object(9 << 16)),
            form: None,
            store: None,
        };
        let (first, counts) = three_glyphs(&source, true);
        assert_eq!(
            (counts.glyph_masks_rasterised, counts.glyph_mask_hits),
            (2, 1),
            "the second glyph's phase is the first's"
        );
        let store = source.store.as_ref().expect("a store");
        assert_eq!(store.asked.lock().expect("unpoisoned").len(), 3);
        assert_eq!(store.masks.lock().expect("unpoisoned").len(), 2);
        let (alone, _) = three_glyphs(&own, true);
        assert!(
            first.data == alone.data,
            "the same pixels as a render's own"
        );

        let (second, counts) = three_glyphs(&source, true);
        assert_eq!(
            (counts.glyph_masks_rasterised, counts.glyph_mask_hits),
            (0, 3),
            "every mask found where the first render kept it"
        );
        assert!(second.data == first.data);

        let (hard, counts) = three_glyphs(&source, false);
        assert_eq!(
            counts.glyph_masks_rasterised, 2,
            "another setting is another key"
        );
        let (hard_alone, _) = three_glyphs(&own, false);
        assert!(hard.data == hard_alone.data);
        let asked = store.asked.lock().expect("unpoisoned");
        assert!(asked[..6]
            .iter()
            .all(|key| key.bits == 8 && key.font_id == 7));
        assert!(asked[6..].iter().all(|key| key.bits == 0));
        assert_eq!(asked[0].tolerance, asked[6].tolerance);
    }
}
