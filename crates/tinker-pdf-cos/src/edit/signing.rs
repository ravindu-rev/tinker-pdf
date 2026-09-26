//! Saving with a signature: the signature field, `/SigFlags`, certification.

use super::DocumentEditor;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::sign::{SignError, SigningRequest, SigningTarget};
use crate::text_string::encode_text_string;
use crate::write::{self, WriteMode, WriteOptions};

impl DocumentEditor {
    /// Saves the edits and signs the result (12.8.1).
    ///
    /// The signature covers every byte of the output except the `/Contents`
    /// string holding it, which is the only coverage that means "this document,
    /// as you have it". Everything the original file already contained is
    /// inside it, because an incremental save leaves the original bytes alone.
    ///
    /// # How the circle is broken
    ///
    /// `/Contents` signs bytes that surround it, so the space is reserved,
    /// the file is finished, `/ByteRange` is patched to describe the finished
    /// file, and only then is the digest taken and the blob written into the
    /// reservation. Patching order is load-bearing: `/ByteRange` is *inside*
    /// the range it describes, so digesting before it was patched would sign
    /// a placeholder.
    ///
    /// # Errors
    /// [`SignError`], including the host's own refusal carried verbatim. A CMS
    /// blob larger than [`SigningRequest::reserve`] is refused rather than
    /// truncated: the reservation cannot grow once the cross-reference table
    /// has recorded every offset around it, and a truncated signature is a
    /// file that looks signed and is not.
    pub fn save_signed(
        &mut self,
        options: &WriteOptions,
        request: &SigningRequest<'_>,
    ) -> Result<Vec<u8>, SignError> {
        if options.mode != WriteMode::Incremental {
            return Err(SignError::NotIncremental);
        }

        let signature_ref = self.allocate();
        match &request.target {
            SigningTarget::Field(name) => self.attach_to_field(name, signature_ref)?,
            SigningTarget::NewInvisibleField { name } => {
                self.attach_to_new_field(name, signature_ref)?;
            }
        }

        // Before the object set is taken, or the catalog's `/Perms` is an edit
        // made and never written — which is what it was: the signature's own
        // `/Reference` still said DocMDP, so reading the certification back
        // did not notice the catalog entry 12.8.4 asks for was missing.
        if request.certification.is_some() {
            self.certify(signature_ref);
        }
        let set = self.changed_set();
        let trailer = self.doc.trailer().clone();
        let key = self.doc.file_key();
        let cipher = key.as_ref().map(|key| write::InheritedCipher { key });
        let catalog = self.doc.trailer().get_ref(Name::ROOT);
        let reserved = crate::sign::Reserved::build(request, catalog);
        let (mut out, placeholder) = write::incremental_update_reserving(
            self.doc.bytes(),
            &set,
            &trailer,
            self.doc.last_startxref(),
            &write::UpdatePlan {
                names: self.doc.names_table(),
                compress: options.compress,
                crypt: cipher.as_ref().map(|c| c as &dyn write::ObjectCipher),
                reserved: Some((signature_ref.num, &reserved)),
            },
        );
        let placeholder = placeholder.ok_or(SignError::RangeDoesNotFit)?;
        crate::sign::seal(&mut out, &placeholder, request.signer)?;
        Ok(out)
    }

    /// 12.8.4: the catalog's `/Perms /DocMDP` names the certifying signature,
    /// which is what lets a reader find the document's certification without
    /// walking every field looking for a `/Reference`.
    fn certify(&mut self, signature: ObjRef) {
        let perms_key = self.intern(b"Perms");
        let docmdp = self.intern(b"DocMDP");
        // An indirect `/Perms` is its own object and is written there.
        if let Some(Object::Ref(perms_ref)) = self.catalog().and_then(|c| c.get(perms_key).cloned())
        {
            if let Some(Object::Dict(mut perms)) = self.get(perms_ref) {
                perms.insert(docmdp, Object::Ref(signature));
                self.put(perms_ref, Object::Dict(perms));
                return;
            }
        }
        self.update_catalog(|catalog| {
            let mut perms = match catalog.get(perms_key) {
                Some(Object::Dict(dict)) => dict.clone(),
                _ => Dict::new(),
            };
            perms.insert(docmdp, Object::Ref(signature));
            catalog.insert(perms_key, Object::Dict(perms));
        });
    }

    /// Points an existing empty signature field at `signature`.
    fn attach_to_field(&mut self, name: &str, signature: ObjRef) -> Result<(), SignError> {
        let field = self
            .fields()
            .into_iter()
            .find(|field| field.name == name)
            .ok_or_else(|| SignError::NoSuchField(name.to_string()))?;
        if field.kind != crate::form::FieldKind::Signature {
            return Err(SignError::NotASignatureField(name.to_string()));
        }
        let value = self.intern(b"V");
        let Some(Object::Dict(mut dict)) = self.get(field.reference) else {
            return Err(SignError::NoSuchField(name.to_string()));
        };
        // Overwriting a signature destroys the evidence it was, and a caller
        // who wanted a second one meant a second field.
        if dict.get(value).is_some() {
            return Err(SignError::FieldAlreadySigned(name.to_string()));
        }
        dict.insert(value, Object::Ref(signature));
        self.put(field.reference, Object::Dict(dict));
        self.register_signature_field(None);
        Ok(())
    }

    /// Adds an invisible signature field on the first page, pointing at
    /// `signature`.
    ///
    /// Invisible — a zero `/Rect` with 12.5.3's NoView bit — because drawing a
    /// signature appearance is a separate capability this build does not have,
    /// and an empty rectangle where a seal should be is worse than nothing
    /// visible at all. `Print` is set with it so the field's absence is
    /// consistent on paper and on screen.
    fn attach_to_new_field(&mut self, name: &str, signature: ObjRef) -> Result<(), SignError> {
        let page = *self.page_refs().first().ok_or(SignError::NoPages)?;
        let widget = self.allocate();

        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.intern(b"Annot")));
        dict.insert(
            self.intern(b"Subtype"),
            Object::Name(self.intern(b"Widget")),
        );
        dict.insert(self.intern(b"FT"), Object::Name(self.intern(b"Sig")));
        dict.insert(
            self.intern(b"T"),
            // 12.7.3.1 Table 226: `/T` is a text string, and the name a
            // later `fields()` matches is its decoding.
            Object::String(encode_text_string(name, self.text_version())),
        );
        dict.insert(self.intern(b"Rect"), Object::Array(vec![Object::Int(0); 4]));
        dict.insert(self.intern(b"F"), Object::Int(132));
        dict.insert(self.intern(b"P"), Object::Ref(page));
        dict.insert(self.intern(b"V"), Object::Ref(signature));
        self.put(widget, Object::Dict(dict));

        self.append_to_page_annots(page, widget);
        self.register_signature_field(Some(widget));
        Ok(())
    }

    fn append_to_page_annots(&mut self, page: ObjRef, widget: ObjRef) {
        let annots = self.intern(b"Annots");
        let Some(Object::Dict(mut dict)) = self.get(page) else {
            return;
        };
        // `/Annots` may be the array itself or a reference to one, and the two
        // are written back to different objects.
        match dict.get(annots).cloned() {
            Some(Object::Ref(array_ref)) => {
                let mut existing = match self.get(array_ref) {
                    Some(Object::Array(items)) => items,
                    _ => Vec::new(),
                };
                existing.push(Object::Ref(widget));
                self.put(array_ref, Object::Array(existing));
            }
            Some(Object::Array(mut existing)) => {
                existing.push(Object::Ref(widget));
                dict.insert(annots, Object::Array(existing));
                self.put(page, Object::Dict(dict));
            }
            _ => {
                dict.insert(annots, Object::Array(vec![Object::Ref(widget)]));
                self.put(page, Object::Dict(dict));
            }
        }
    }

    /// Adds `widget` to `/Fields` and sets `/SigFlags`, in one update.
    ///
    /// 12.7.2 Table 218: `/SigFlags` bit 1 says the document has a signature
    /// and bit 2 says it must only ever be saved incrementally. Both, because
    /// both are true of a file this method produced.
    fn register_signature_field(&mut self, widget: Option<ObjRef>) {
        let Some((home, mut form)) = self.acroform() else {
            return;
        };
        if let Some(widget) = widget {
            let fields = self.intern(b"Fields");
            let mut list = match form.get(fields).cloned() {
                Some(Object::Array(items)) => items,
                Some(Object::Ref(array_ref)) => match self.get(array_ref) {
                    Some(Object::Array(items)) => items,
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            };
            list.push(Object::Ref(widget));
            form.insert(fields, Object::Array(list));
        }
        let flags = self.intern(b"SigFlags");
        form.insert(flags, Object::Int(3));
        self.put_acroform(home, form);
    }
}
