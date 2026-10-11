//! Tagged writing: structure elements opened and closed on a page
//! (`PageBuilder::open_tag` / `close_tag`), the document's `/Lang` and its
//! role map. One facade call each (ruling 11). A `PdfTag` is built with
//! setters and borrowed by `openTag`, so one tag may open several elements;
//! a key and order are the facade's `u64`s, so `BigInt`s.

use wasm_bindgen::prelude::*;

use tinker_pdf::Tag;

use crate::{refused, PdfBuilder, PdfPageBuilder};

/// A structure element to open (14.7.2): its type and properties.
#[wasm_bindgen]
pub struct PdfTag {
    inner: Tag,
}

impl PdfTag {
    fn rebuild(&mut self, with: impl FnOnce(Tag) -> Tag) {
        let taken = std::mem::replace(&mut self.inner, Tag::new(b""));
        self.inner = with(taken);
    }
}

#[wasm_bindgen]
impl PdfTag {
    /// An element of structure type `kind`: `P`, `H1`, `Figure`, or a type
    /// mapped with `mapRole`.
    #[wasm_bindgen(constructor)]
    pub fn new(kind: &[u8]) -> PdfTag {
        PdfTag {
            inner: Tag::new(kind),
        }
    }

    /// `/T`, the element's title.
    #[wasm_bindgen(js_name = setTitle)]
    pub fn set_title(&mut self, text: &str) {
        self.rebuild(|tag| tag.title(text));
    }

    /// `/Lang`: a BCP 47 tag, or `""` for unknown.
    #[wasm_bindgen(js_name = setLang)]
    pub fn set_lang(&mut self, text: &str) {
        self.rebuild(|tag| tag.lang(text));
    }

    /// `/Alt`, a description of content that is not text.
    #[wasm_bindgen(js_name = setAlt)]
    pub fn set_alt(&mut self, text: &str) {
        self.rebuild(|tag| tag.alt(text));
    }

    /// `/ActualText`, what the content is where the glyphs do not say it.
    #[wasm_bindgen(js_name = setActualText)]
    pub fn set_actual_text(&mut self, text: &str) {
        self.rebuild(|tag| tag.actual_text(text));
    }

    /// `/E`, the expansion of an abbreviation.
    #[wasm_bindgen(js_name = setExpansion)]
    pub fn set_expansion(&mut self, text: &str) {
        self.rebuild(|tag| tag.expansion(text));
    }

    /// `/ID`, the element's identifier.
    #[wasm_bindgen(js_name = setId)]
    pub fn set_id(&mut self, id: &[u8]) {
        self.rebuild(|tag| tag.id(id));
    }

    /// Names the element so its halves drawn apart are one element: `key`
    /// says which, `order` where this half reads.
    #[wasm_bindgen(js_name = setKey)]
    pub fn set_key(&mut self, key: u64, order: u64) {
        self.rebuild(|tag| tag.keyed(key, order));
    }

    /// Writes the element even with nothing drawn inside it.
    #[wasm_bindgen(js_name = keepEmpty)]
    pub fn keep_empty(&mut self) {
        self.rebuild(Tag::keep_empty);
    }
}

#[wasm_bindgen]
impl PdfPageBuilder {
    /// Opens the element `tag` describes, until the matching `closeTag` --
    /// across calls and pages. Throws, opening nothing, past the deepest
    /// nesting the reader walks; the close is still owed.
    #[wasm_bindgen(js_name = openTag)]
    pub fn open_tag(&mut self, tag: &PdfTag) -> Result<(), JsError> {
        if self.get()?.open_tag(&tag.inner) {
            Ok(())
        } else {
            Err(refused(
                "openTag",
                "past the deepest nesting this engine reads back",
            ))
        }
    }

    /// Closes the innermost element `openTag` opened; throws when none is.
    #[wasm_bindgen(js_name = closeTag)]
    pub fn close_tag(&mut self) -> Result<(), JsError> {
        if self.get()?.close_tag() {
            Ok(())
        } else {
            Err(refused("closeTag", "no element is open"))
        }
    }
}

#[wasm_bindgen]
impl PdfBuilder {
    /// Sets the catalog's `/Lang`: a BCP 47 tag, or `""` for unknown.
    #[wasm_bindgen(js_name = setLanguage)]
    pub fn set_language(&mut self, language: &str) -> Result<(), JsError> {
        self.get()?.set_language(language);
        Ok(())
    }

    /// Maps a structure type of the caller's own to a standard one; throws,
    /// mapping nothing, when the facade refuses it.
    #[wasm_bindgen(js_name = mapRole)]
    pub fn map_role(&mut self, custom: &[u8], standard: &[u8]) -> Result<(), JsError> {
        if self.get()?.map_role(custom, standard) {
            Ok(())
        } else {
            Err(refused(
                "mapRole",
                &format!(
                    "{:?} to {:?}",
                    String::from_utf8_lossy(custom),
                    String::from_utf8_lossy(standard)
                ),
            ))
        }
    }
}
