//! Saving with a signature: the signature field, `/SigFlags`, certification.

use super::DocumentEditor;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::pages::Rect;
use crate::sign::{SignError, SignatureAppearance, SignatureImage, SigningRequest, SigningTarget};
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
            SigningTarget::NewVisibleField {
                name,
                page,
                rect,
                appearance,
            } => {
                let seal = Seal {
                    page: *page,
                    rect: *rect,
                    appearance,
                    lines: seal_lines(request),
                };
                self.attach_to_new_visible_field(name, &seal, signature_ref)?;
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
        let trailer = self.update_trailer();
        let key = self.doc.file_key();
        let cipher = key.as_ref().map(|key| write::InheritedCipher { key });
        let catalog = trailer.get_ref(Name::ROOT);
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
    /// Invisible because its `/Rect` is zero, for a caller who asked for a
    /// signature and no seal; an empty rectangle where a seal should be is
    /// worse than nothing visible at all. [`SigningTarget::NewVisibleField`]
    /// is the one that draws.
    ///
    /// `/F 132` is Print and Locked (12.5.3 Table 165, bits 3 and 8), the
    /// convention invisible signatures carry. This comment said until
    /// September 2026 that it was "12.5.3's NoView bit", which is bit 6 and
    /// 32; nothing was ever hidden by a flag, only by the empty rectangle.
    fn attach_to_new_field(&mut self, name: &str, signature: ObjRef) -> Result<(), SignError> {
        let page = *self.page_refs().first().ok_or(SignError::NoPages)?;
        let widget = self.allocate();

        let dict = self.signature_widget(
            name,
            page,
            Object::Array(vec![Object::Int(0); 4]),
            132,
            signature,
        );
        self.put(widget, Object::Dict(dict));

        self.append_to_page_annots(page, widget);
        self.register_signature_field(Some(widget));
        Ok(())
    }

    /// A signature field merged with its widget (12.7.4.5, 12.7.3.3).
    fn signature_widget(
        &self,
        name: &str,
        page: ObjRef,
        rect: Object,
        flags: i64,
        signature: ObjRef,
    ) -> Dict {
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
        dict.insert(self.intern(b"Rect"), rect);
        dict.insert(self.intern(b"F"), Object::Int(flags));
        dict.insert(self.intern(b"P"), Object::Ref(page));
        dict.insert(self.intern(b"V"), Object::Ref(signature));
        dict
    }

    /// Adds a signature field whose widget draws a seal, pointing at
    /// `signature`.
    ///
    /// Everything that can refuse — the page, the rectangle, the image — is
    /// checked before anything is written. The appearance is built by
    /// [`crate::appearance::signature`], beside the synthesis every other
    /// annotation's appearance comes from, and it is an object of the same
    /// update as the signature dictionary: the `/ByteRange` covers it.
    fn attach_to_new_visible_field(
        &mut self,
        name: &str,
        seal: &Seal<'_>,
        signature: ObjRef,
    ) -> Result<(), SignError> {
        let page = *self
            .page_refs()
            .get(seal.page as usize)
            .ok_or(SignError::NoSuchPage(seal.page))?;
        let rect = super::forms::usable_rect(seal.rect).ok_or(SignError::RectUnusable)?;
        let image = match &seal.appearance.image {
            Some(image) => Some(self.signature_image(image)?),
            None => None,
        };

        let image = image.map(|(stream, width, height)| {
            let r = self.allocate();
            self.put_stream(r, stream);
            (r, width, height)
        });
        let mut unwritable = Vec::new();
        let form = crate::appearance::signature(
            &self.doc,
            rect.x1 - rect.x0,
            rect.y1 - rect.y0,
            &seal.lines,
            image,
            &mut unwritable,
        );
        let form_ref = self.allocate();
        self.put_stream(form_ref, form);

        let widget = self.allocate();
        let mut dict = self.signature_widget(
            name,
            page,
            Object::Array(vec![
                Object::Real(rect.x0),
                Object::Real(rect.y0),
                Object::Real(rect.x1),
                Object::Real(rect.y1),
            ]),
            // 12.5.3 Table 165: Print, so the seal is on paper too.
            4,
            signature,
        );
        let mut ap = Dict::new();
        ap.insert(self.intern(b"N"), Object::Ref(form_ref));
        dict.insert(self.intern(b"AP"), Object::Dict(ap));
        self.put(widget, Object::Dict(dict));

        // Ruling 10: a character the seal drew as `?` is named against the
        // widget it was drawn in.
        if !unwritable.is_empty() {
            let mut sink = crate::warn::WarningSink::new();
            sink.set_context(Some(widget));
            for character in unwritable {
                sink.warn(
                    0,
                    crate::warn::WarningKind::FieldCharacterUnrepresentable { character },
                );
            }
            self.doc.absorb(sink);
        }

        self.append_to_page_annots(page, widget);
        self.register_signature_field(Some(widget));
        Ok(())
    }

    /// An image XObject (8.9.5) for a seal, or the reason it is not one.
    ///
    /// The editor's minimal counterpart of `DocumentBuilder::add_image`: a
    /// JPEG placed as it is with its shape read from its own frame header,
    /// and eight-bit samples placed as they are.
    fn signature_image(
        &self,
        image: &SignatureImage,
    ) -> Result<(crate::write::StreamData, u32, u32), SignError> {
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.intern(b"XObject")));
        dict.insert(self.intern(b"Subtype"), Object::Name(self.intern(b"Image")));
        let (width, height, space, data): (u32, u32, &[u8], Vec<u8>) = match image {
            SignatureImage::Jpeg(bytes) => {
                let (width, height, components) =
                    crate::build::jpeg_shape(bytes).ok_or(SignError::ImageUnusable)?;
                dict.insert(Name::FILTER, Object::Name(self.intern(b"DCTDecode")));
                let space: &[u8] = match components {
                    1 => b"DeviceGray",
                    4 => b"DeviceCMYK",
                    _ => b"DeviceRGB",
                };
                (width, height, space, bytes.clone())
            }
            SignatureImage::Gray8 {
                width,
                height,
                data,
            }
            | SignatureImage::Rgb8 {
                width,
                height,
                data,
            } => {
                let gray = matches!(image, SignatureImage::Gray8 { .. });
                let per_pixel: u64 = if gray { 1 } else { 3 };
                let expected = u64::from(*width)
                    .saturating_mul(u64::from(*height))
                    .saturating_mul(per_pixel);
                let expected = usize::try_from(expected).map_err(|_| SignError::ImageUnusable)?;
                let samples = data.get(..expected).ok_or(SignError::ImageUnusable)?;
                let space: &[u8] = if gray { b"DeviceGray" } else { b"DeviceRGB" };
                (*width, *height, space, samples.to_vec())
            }
        };
        if width == 0 || height == 0 {
            return Err(SignError::ImageUnusable);
        }
        dict.insert(self.intern(b"Width"), Object::Int(i64::from(width)));
        dict.insert(self.intern(b"Height"), Object::Int(i64::from(height)));
        dict.insert(self.intern(b"BitsPerComponent"), Object::Int(8));
        dict.insert(self.intern(b"ColorSpace"), Object::Name(self.intern(space)));
        Ok((crate::write::StreamData { dict, data }, width, height))
    }

    pub(super) fn append_to_page_annots(&mut self, page: ObjRef, widget: ObjRef) {
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

/// What a visible signature draws, and where.
struct Seal<'a> {
    page: u32,
    rect: Rect,
    appearance: &'a SignatureAppearance,
    lines: Vec<String>,
}

/// The seal's text, from the request's own entries — who, when, why, where —
/// so that what the seal says and what the signature dictionary says are one
/// statement rather than two that can drift.
///
/// A control character in a caller's string becomes a space: a line break
/// inside a literal string is a byte a content stream draws as nothing, and
/// silently dropping it would run two words together.
fn seal_lines(request: &SigningRequest<'_>) -> Vec<String> {
    let clean = |text: &str| -> String {
        text.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };
    let mut lines = vec![match &request.name {
        Some(name) => format!("Digitally signed by {}", clean(name)),
        None => "Digitally signed".to_string(),
    }];
    if let Some(date) = request.signed_at {
        lines.push(format!("Date: {}", crate::sign::display_date(date)));
    }
    if let Some(reason) = &request.reason {
        lines.push(format!("Reason: {}", clean(reason)));
    }
    if let Some(location) = &request.location {
        lines.push(format!("Location: {}", clean(location)));
    }
    lines
}
