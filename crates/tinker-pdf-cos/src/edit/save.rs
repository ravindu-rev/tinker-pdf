//! Serialising the edits: an incremental update or a whole rewrite.

use std::collections::{BTreeMap, HashSet};

use super::{without, DocumentEditor};
use crate::limits;
use crate::object::{Dict, ObjRef, Object};
use crate::resolve::Resolve;
use crate::write::{self, ObjectSet, StreamData, WriteMode, WriteOptions, Written};
use tinker_pdf_crypto::Permissions;

/// Every indirect reference inside `value`, not following any, nested no
/// deeper than [`limits::MAX_NEST_DEPTH`].
pub(super) fn refs_in(value: &Object, out: &mut Vec<ObjRef>, depth: u32) {
    if depth > limits::MAX_NEST_DEPTH {
        return;
    }
    match value {
        Object::Ref(r) => out.push(*r),
        Object::Array(items) => {
            for item in items {
                refs_in(item, out, depth + 1);
            }
        }
        Object::Dict(dict) => {
            for (_, item) in dict.iter() {
                refs_in(item, out, depth + 1);
            }
        }
        Object::Stream(stream) => {
            for (_, item) in stream.dict.iter() {
                refs_in(item, out, depth + 1);
            }
        }
        _ => {}
    }
}

/// Every indirect reference inside an object, pushed onto `queue`.
fn references_of(object: &Object, queue: &mut Vec<ObjRef>) {
    match object {
        Object::Ref(r) => queue.push(*r),
        Object::Array(items) => {
            for item in items {
                references_of(item, queue);
            }
        }
        Object::Dict(dict) => {
            for (_, value) in dict.iter() {
                references_of(value, queue);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                references_of(value, queue);
            }
        }
        _ => {}
    }
}

impl DocumentEditor {
    /// Every object a save of this editor reaches from `trailer`, reading each
    /// object as `pending` has it where it has it, and as the editor does
    /// otherwise — for an edit deciding what it may delete.
    ///
    /// The page order counts. A page [`DocumentEditor::insert_page`] or
    /// [`DocumentEditor::import_page`] put in it is in the document, though
    /// only a save writes it into `/Kids` ([`DocumentEditor::page_tree_updates`]),
    /// and a walk that read `/Kids` as the editor has it found that page, and
    /// everything it draws, nowhere — which is how sanitising once deleted an
    /// imported page's content. A page the order dropped is still found
    /// through the old `/Kids`, so the answer can hold more than a save keeps
    /// and never less: the safe side for a caller deciding what to delete.
    pub(super) fn reached(&self, trailer: &Dict, pending: &BTreeMap<u32, Object>) -> HashSet<u32> {
        let mut live = HashSet::new();
        let mut queue = Vec::new();
        for (_, value) in trailer.iter() {
            refs_in(value, &mut queue, 0);
        }
        if let Some(order) = &self.page_order {
            queue.extend(order.iter().copied());
        }
        while let Some(r) = queue.pop() {
            if !live.insert(r.num) {
                continue;
            }
            match pending.get(&r.num) {
                Some(object) => refs_in(object, &mut queue, 0),
                None => {
                    if let Ok(object) = Resolve::get(self, r) {
                        refs_in(&object, &mut queue, 0);
                    }
                }
            }
        }
        live
    }

    /// Keeps only the objects something reaches from the trailer.
    ///
    /// A mark from the trailer's own references, then a sweep. Without it a
    /// rewrite is a serializer rather than a rewrite: an object detached from
    /// the page tree is still written, still numbered, and still readable by
    /// anyone who scans the file rather than following it — which is how
    /// "deleted" content stays recoverable.
    fn reachable(&self, all: &ObjectSet, trailer: &Dict) -> ObjectSet {
        let mut live: HashSet<u32> = HashSet::new();
        let mut queue: Vec<ObjRef> = Vec::new();

        for (_, value) in trailer.iter() {
            references_of(value, &mut queue);
        }

        while let Some(r) = queue.pop() {
            if !live.insert(r.num) {
                continue;
            }
            let Some(entry) = all.get(r.num) else {
                continue;
            };
            let dict = match entry {
                Written::Object(object) => {
                    references_of(object, &mut queue);
                    continue;
                }
                Written::Stream(stream) => &stream.dict,
            };
            for (_, value) in dict.iter() {
                references_of(value, &mut queue);
            }
        }

        let mut kept = ObjectSet::new();
        for (num, entry) in all.iter() {
            if !live.contains(num) {
                continue;
            }
            match entry {
                Written::Object(object) => kept.insert(*num, object.clone()),
                Written::Stream(stream) => kept.insert_stream(*num, stream.clone()),
            }
        }
        kept
    }

    /// Whether a save with `options` would undo what encryption protects
    /// without being asked to, or with less authority than that takes — the
    /// two decisions `tpdf` used to make on its own, made here so that every
    /// surface can ask the same question.
    ///
    /// [`DocumentEditor::save`] does not ask it: a save returns bytes, and
    /// whether the save door itself refuses is a change to that door's
    /// contract, which the ROADMAP's CLI row leaves to the owner. This is the
    /// question, for a caller that wants the answer before it writes.
    ///
    /// # Errors
    ///
    /// - [`SaveRefusal::WouldDecrypt`] when the save is a rewrite asking for
    ///   no encryption of a document that is encrypted, or that anything was
    ///   copied into from an encrypted one — the rewrite drops `/Encrypt` and
    ///   writes the plaintext — or an incremental update of a document that
    ///   something was copied into from an encrypted one and that holds no
    ///   key that seals it. An update is sealed with the key the document
    ///   was opened with (7.6.2) and with nothing else:
    ///   [`WriteOptions::encryption`] is not read for one, so it seals
    ///   nothing there; a document that is not encrypted, or was opened
    ///   without its password, has no key; and a key whose `/StmF` or `/StrF`
    ///   is `/Identity` passes those streams or strings through unchanged
    ///   (7.6.5 Table 25), as the document's own are stored, and so does an
    ///   AES method under a key AES does not take — `/AESV2` with a 40-bit
    ///   key gets a ten-byte one from Algorithm 1 (7.6.2) — which
    ///   [`tinker_pdf_crypto::FileKey::seals`] asks of the encryption itself;
    /// - [`SaveRefusal::OwnerAuthorityNeeded`] when the document is encrypted,
    ///   was opened with the user's authority, the owner withholds permissions
    ///   from that user (7.6.4.2, Table 22), and the save is a rewrite that
    ///   replaces the encryption — which lifts what the owner withheld. An
    ///   incremental update leaves the document's own `/Encrypt` standing,
    ///   whatever encryption it was asked for. Permissions are
    ///   advisory and this crate reports rather than enforces them
    ///   ([`crate::CosDocument::permissions`]); this is the one operation that
    ///   would erase them, and it honours them instead.
    ///
    /// Decrypting on purpose is [`DocumentEditor::check_decrypt`]'s question.
    pub fn check_save(&self, options: &WriteOptions) -> Result<(), SaveRefusal> {
        let encrypted = self.doc.is_encrypted();
        // Held to what `save_with` does, arm by arm.
        let would_decrypt = match options.mode {
            // A rewrite drops `/Encrypt` and is sealed with the encryption it
            // is asked for or with none, so replacing the document's own
            // encryption is the one place the owner's authority is needed.
            WriteMode::Rewrite => {
                if encrypted && options.encryption.is_some() {
                    self.owner_authority()?;
                }
                options.encryption.is_none() && (encrypted || self.encrypted_source)
            }
            // An update is sealed with the key the document was opened with
            // and with nothing else: it reads no `WriteOptions::encryption`,
            // and with no key it writes in the clear. So it replaces no
            // encryption and lifts nothing the owner withheld, and what it
            // can write decrypted is only what was copied in — the
            // document's own objects it writes as they were read, which for
            // one opened without its password is still ciphertext. Counting
            // the encryption asked of an update as a seal, or the document's
            // `/Encrypt` as one whatever key it was opened with, answered
            // `Ok` to updates that wrote an encrypted source's plaintext; so
            // did counting any key as one, when a key whose stream or string
            // method is `/Identity` writes that half in the clear, and so did
            // counting any key whose methods are ciphers, when AES under a
            // key it does not take hands back what it was given.
            WriteMode::Incremental => {
                self.encrypted_source && !self.doc.file_key().is_some_and(|key| key.seals())
            }
        };
        if would_decrypt {
            return Err(SaveRefusal::WouldDecrypt);
        }
        Ok(())
    }

    /// Whether this document may be written decrypted on purpose: it is
    /// encrypted, and whoever opened it holds the owner's authority or was
    /// withheld nothing (7.6.4.2).
    ///
    /// # Errors
    ///
    /// [`SaveRefusal::NotEncrypted`] for a document with nothing to decrypt,
    /// and [`SaveRefusal::OwnerAuthorityNeeded`] as
    /// [`DocumentEditor::check_save`] gives it: removing the encryption
    /// lifts what the owner withheld.
    pub fn check_decrypt(&self) -> Result<(), SaveRefusal> {
        if !self.doc.is_encrypted() {
            return Err(SaveRefusal::NotEncrypted);
        }
        self.owner_authority()
    }

    /// The owner's authority, or a user the owner withheld nothing from.
    fn owner_authority(&self) -> Result<(), SaveRefusal> {
        // `permissions()` answers "everything" for the owner, so nothing is
        // withheld exactly when the owner's authority is what opened it, or
        // the owner restricted the user in nothing.
        let permissions = self.doc.permissions();
        if withheld(permissions).is_empty() {
            Ok(())
        } else {
            Err(SaveRefusal::OwnerAuthorityNeeded { permissions })
        }
    }

    /// Saves the edits.
    ///
    /// An incremental save appends only what changed, leaving the original
    /// bytes untouched — the only way to modify a signed document without
    /// breaking the signature over it.
    ///
    /// It writes what it is asked to: [`DocumentEditor::check_save`] is the
    /// question of whether that undoes an encryption.
    #[must_use]
    pub fn save(&self, options: &WriteOptions) -> Vec<u8> {
        self.save_with(options, write::Sealing::from_options(options))
    }

    /// Saves the edits as a document **sealed to recipients** rather than to
    /// a password (ISO 32000-2 7.6.5): a rewrite whose `/Encrypt` is the
    /// public-key handler's, `/Recipients` the envelopes `sealed` carries, and
    /// whose strings and streams are encrypted under the key derived from
    /// them, exactly as a password-encrypted rewrite's are under its own.
    ///
    /// Beside [`DocumentEditor::save`] rather than inside
    /// [`WriteOptions`], for two reasons recorded in
    /// `docs/features/encryption.md`: `Encryption` is a struct its callers
    /// build by its fields, so a second scheme in it would break every one
    /// of them; and `save` has no error to return, where a sealed save has
    /// two that are the caller's to hear about. The sealing itself — every
    /// certificate read, every envelope written — happened in
    /// [`crate::PublicKeyEncryption::seal`], before this was called.
    ///
    /// # Errors
    /// [`crate::SealError::NotRewrite`] for an incremental save, and
    /// [`crate::SealError::PasswordAlsoRequested`] when
    /// [`WriteOptions::encryption`] is set too.
    pub fn save_sealed(
        &self,
        options: &WriteOptions,
        sealed: &crate::pubsec::PublicKeyEncryption,
    ) -> Result<Vec<u8>, crate::pubsec::SealError> {
        if options.mode != WriteMode::Rewrite {
            return Err(crate::pubsec::SealError::NotRewrite);
        }
        if options.encryption.is_some() {
            return Err(crate::pubsec::SealError::PasswordAlsoRequested);
        }
        Ok(self.save_with(options, Some(write::Sealing::PublicKey(sealed))))
    }

    fn save_with(&self, options: &WriteOptions, sealing: Option<write::Sealing<'_>>) -> Vec<u8> {
        let set = self.changed_set();

        // A reordered page tree needs its /Kids and /Count rewritten.
        //
        // Computed once and applied to whichever object set the chosen mode
        // builds. Writing it only into the incremental set — which is what
        // this did — meant a *rewrite* silently dropped every page operation:
        // the tree kept its original /Kids, so an inserted page was written
        // into the file and never referenced, and a reordering did nothing at
        // all. Nothing caught it because the page-operation tests all saved
        // incrementally.
        //
        // `changed_set` has already folded these into the incremental set;
        // the binding stays because the rewrite arm below needs them too.
        let tree_updates = self.page_tree_updates();

        // The document's trailer with this editor's entries over it, for both
        // modes: an `/Info` created here is named by it or by nothing.
        let trailer = self.merged_trailer();
        match options.mode {
            WriteMode::Incremental => {
                // With every entry removed here written as null, so an earlier
                // revision's trailer does not bring it back.
                let trailer = self.update_trailer();
                // 7.6.2: the update is sealed with the key the document was
                // opened with, because it appends into a file whose /Encrypt
                // still stands. An unencrypted document, or one never
                // authenticated, has no key and writes in the clear as before;
                // a key whose stream or string method is `/Identity` writes
                // that half in the clear, as the file stores its own
                // (`check_save` names what that writes of an import).
                let key = self.doc.file_key();
                let cipher = key.as_ref().map(|key| write::InheritedCipher { key });
                write::incremental_update(
                    self.doc.bytes(),
                    &set,
                    &trailer,
                    self.doc.last_startxref(),
                    self.doc.names_table(),
                    options.compress,
                    cipher.as_ref().map(|c| c as &dyn write::ObjectCipher),
                )
            }
            WriteMode::Rewrite => {
                // 7.6.1: a rewrite decrypts on the way through — `stream_raw`
                // hands back plaintext once a decryptor is installed, and the
                // strings were decrypted when they were parsed. So the output
                // is a plaintext file, and carrying `/Encrypt` forward would
                // advertise encryption over it: every reader would then try to
                // decrypt bytes that are already clear and get garbage.
                //
                // Encrypt-on-save is a separate, unbuilt feature. Until it
                // exists, writing a readable file is the honest outcome, and
                // it is what `WriteOptions::encryption` will replace.
                let encrypt = self.intern(b"Encrypt");
                let trailer = without(&trailer, encrypt);
                let encrypt_num = self.doc.trailer().get_ref(encrypt).map(|r| r.num);

                // A rewrite must carry everything, not only the changes.
                //
                // Over the object numbers the document **has**, not over the
                // range its numbering allows. This walked `1..=max_object_
                // number()`, which is the same thing for almost every file and
                // two thousand million lookups for
                // `pdfjs/test/pdfs/bug1980958.pdf` — 219 bytes, four objects,
                // the last of them numbered `i32::MAX`. A save over it had not
                // returned after three minutes. The reader was hardened
                // against exactly this shape years ago (`limits::MAX_XREF_
                // SLOTS`, dense below the cap and a map above it); the editor
                // never was, and nothing noticed because the corpus runner
                // reads a hang as a slow file.
                //
                // `XrefTable::iter` yields ascending object numbers across
                // both halves of that table, so the objects are visited in the
                // same order as before and the bytes a rewrite produces are
                // unchanged for every file whose numbering is dense.
                let mut all = ObjectSet::new();
                for (num, _) in self.doc.xref().iter() {
                    if num == 0 {
                        // Object zero is the head of the free list, never a
                        // real object; the old range started at one.
                        continue;
                    }
                    let r = ObjRef::new(num, 0);
                    if self.deleted.contains(&num) {
                        continue;
                    }
                    // The encryption dictionary itself goes with the entry
                    // that named it; left behind it is an orphan describing a
                    // scheme the file no longer uses.
                    if encrypt_num == Some(num) {
                        continue;
                    }
                    match self.overlay.get(&num) {
                        Some(Written::Object(object)) => all.insert(num, object.clone()),
                        Some(Written::Stream(stream)) => all.insert_stream(num, stream.clone()),
                        None => {
                            if let Ok(object) = self.doc.get(r) {
                                if matches!(object.as_ref(), Object::Null) {
                                    continue;
                                }
                                // A stream must carry its data, which the
                                // parsed object does not hold.
                                if let Object::Stream(stream) = object.as_ref() {
                                    if let Ok(data) = self.doc.stream_raw(r) {
                                        all.insert_stream(
                                            num,
                                            StreamData {
                                                dict: stream.dict.clone(),
                                                data,
                                            },
                                        );
                                        continue;
                                    }
                                }
                                all.insert(num, (*object).clone());
                            }
                        }
                    }
                }
                for (num, entry) in &self.overlay {
                    match entry {
                        Written::Object(object) => all.insert(*num, object.clone()),
                        Written::Stream(stream) => all.insert_stream(*num, stream.clone()),
                    }
                }
                for (num, object) in &tree_updates {
                    all.insert(*num, object.clone());
                }
                if options.garbage_collect {
                    all = self.reachable(&all, &trailer);
                }
                let mut trailer = trailer;
                if options.deduplicate_streams {
                    let content = |num: u32, stream: &StreamData| self.dedup_content(num, stream);
                    let (merged, redirected, _) = crate::dedup::deduplicate(
                        &all,
                        &trailer,
                        self.doc.names_table(),
                        &content,
                        &tinker_pdf_crypto::sha2::sha256,
                    );
                    all = merged;
                    trailer = redirected;
                }
                write::rewrite_sealed(&all, &trailer, options, self.doc.names_table(), sealing)
            }
        }
    }

    /// A stream's content as stream deduplication compares it: decoded
    /// through its whole `/Filter` chain when that is exact, and its stored
    /// bytes when it is not.
    ///
    /// "Exact" is a decode that raised no warning at all: a filter this build
    /// cannot run, an image codec the chain stops at, damage, and the output
    /// cap each warn, and a truncated decode compared as if whole could make
    /// two different streams look alike. The warnings go nowhere: a save that
    /// changed what `CosDocument::warnings` reports would make the document's
    /// answer depend on whether it had been saved.
    fn dedup_content(&self, num: u32, stream: &StreamData) -> crate::dedup::Content {
        use crate::dedup::Content;
        if stream.dict.get(crate::name::Name::FILTER).is_none() {
            return Content::Decoded(stream.data.clone());
        }
        let mut sink = crate::warn::WarningSink::new();
        match self
            .doc
            .decode_with(&stream.data, &stream.dict, num, &mut sink)
        {
            Ok(decoded) if sink.is_empty() => Content::Decoded(decoded),
            _ => Content::Stored(stream.data.clone()),
        }
    }

    /// The overlay, deletions and page-tree rewrites as one object set.
    ///
    /// Factored out of [`DocumentEditor::save`] so that signing writes exactly
    /// the same objects an ordinary incremental save would, rather than a
    /// second assembly of them that could drift.
    pub(super) fn changed_set(&self) -> ObjectSet {
        let mut set = ObjectSet::new();
        for (num, entry) in &self.overlay {
            match entry {
                Written::Object(object) => set.insert(*num, object.clone()),
                Written::Stream(stream) => set.insert_stream(*num, stream.clone()),
            }
        }
        for num in &self.deleted {
            set.insert(*num, Object::Null);
        }
        for (num, object) in &self.page_tree_updates() {
            set.insert(*num, object.clone());
        }
        set
    }
}

#[cfg(test)]
mod gc_tests {
    use std::sync::Arc;

    use super::*;
    use crate::build::DocumentBuilder;
    use crate::doc::CosDocument;
    use crate::pages;

    fn document_with_orphan() -> (Arc<CosDocument>, ObjRef) {
        let mut builder = DocumentBuilder::new();
        builder.add_page(100.0, 100.0, |page| {
            page.fill_rect(0.0, 0.0, 10.0, 10.0, 0.0);
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let orphan = editor.allocate();
        editor.put_stream(
            orphan,
            StreamData {
                dict: Dict::new(),
                data: b"THIS-IS-UNREFERENCED".to_vec(),
            },
        );
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        (
            Arc::new(CosDocument::open(saved).expect("it reopens")),
            orphan,
        )
    }

    /// The default is unchanged: a rewrite serializes everything, which is
    /// what callers that overwrite objects in place depend on.
    #[test]
    fn a_rewrite_keeps_orphans_by_default() {
        let (doc, orphan) = document_with_orphan();
        let editor = DocumentEditor::new(Arc::clone(&doc));
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        let text = String::from_utf8_lossy(&saved).into_owned();
        assert!(
            text.contains("THIS-IS-UNREFERENCED"),
            "orphan {orphan:?} kept"
        );
    }

    /// With collection on, nothing the trailer cannot reach survives. An
    /// object detached from the page tree is still readable by anyone who
    /// scans the file rather than following it.
    #[test]
    fn collection_drops_what_nothing_references() {
        let (doc, _) = document_with_orphan();
        let editor = DocumentEditor::new(Arc::clone(&doc));
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            garbage_collect: true,
            ..WriteOptions::default()
        });

        let text = String::from_utf8_lossy(&saved).into_owned();
        assert!(
            !text.contains("THIS-IS-UNREFERENCED"),
            "the orphan survived collection"
        );

        // And the document is still whole.
        let reopened = CosDocument::open(saved).expect("it reopens");
        let pages = pages::collect(&reopened);
        assert_eq!(pages.len(), 1);
        assert!(!pages::content_bytes(&reopened, &pages[0]).is_empty());
    }

    /// Collection must follow references through arrays and nested
    /// dictionaries, not only through the trailer's direct entries.
    #[test]
    fn collection_follows_references_through_nesting() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(100.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, "reachable");
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

        let editor = DocumentEditor::new(Arc::clone(&doc));
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            garbage_collect: true,
            ..WriteOptions::default()
        });

        let reopened = CosDocument::open(saved).expect("it reopens");
        let pages = pages::collect(&reopened);
        assert_eq!(pages.len(), 1, "the page tree survived");
        // The font is reached only through the page's /Resources /Font.
        let resources = pages[0].resources.as_ref().expect("resources survived");
        let fonts = reopened.resolve_key(resources, reopened.intern(b"Font"));
        assert!(
            fonts.as_dict().is_some_and(|d| !d.is_empty()),
            "a font reached through two levels of nesting was collected"
        );
    }
}

/// Why [`DocumentEditor::check_save`] or [`DocumentEditor::check_decrypt`]
/// answers no.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SaveRefusal {
    /// The save would write an encrypted document's plaintext — or what was
    /// copied in from one — with no encryption, without being asked to
    /// decrypt.
    WouldDecrypt,
    /// The save would replace or remove the encryption of a document opened
    /// with the user's authority, from whom the owner withholds permissions:
    /// `permissions` is what the user holds, and [`SaveRefusal::withheld`]
    /// names the rest.
    OwnerAuthorityNeeded {
        /// The permissions the user holds (Table 22).
        permissions: Permissions,
    },
    /// [`DocumentEditor::check_decrypt`] on a document that is not
    /// encrypted.
    NotEncrypted,
}

impl SaveRefusal {
    /// The Table 22 permissions the owner withholds, by name, in the
    /// table's order; empty for every other refusal.
    #[must_use]
    pub fn withheld(&self) -> Vec<&'static str> {
        match self {
            SaveRefusal::OwnerAuthorityNeeded { permissions } => withheld(*permissions),
            _ => Vec::new(),
        }
    }
}

impl std::fmt::Display for SaveRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveRefusal::WouldDecrypt => f.write_str(
                "the save would write an encrypted document decrypted without being asked to",
            ),
            SaveRefusal::OwnerAuthorityNeeded { .. } => write!(
                f,
                "opened as its user, from whom the owner withholds {}; changing the \
                 encryption would lift that, so it needs the owner password",
                self.withheld().join(", ")
            ),
            SaveRefusal::NotEncrypted => f.write_str("the document is not encrypted"),
        }
    }
}

impl std::error::Error for SaveRefusal {}

/// Every named bit of Table 22 a set of permissions does not grant.
fn withheld(p: Permissions) -> Vec<&'static str> {
    [
        ("print", p.print()),
        ("modify", p.modify()),
        ("copy", p.copy()),
        ("annotate", p.annotate()),
        ("fill forms", p.fill_forms()),
        ("extract for accessibility", p.accessibility()),
        ("assemble", p.assemble()),
        ("print at high resolution", p.print_high_res()),
    ]
    .into_iter()
    .filter(|(_, granted)| !granted)
    .map(|(name, _)| name)
    .collect()
}
