//! Tagged writing: structure elements opened and closed on a page as
//! `PageBuilder::open_tag` and `close_tag` -- the closure-free spelling of
//! `PageBuilder::tagged`, which is what crosses an ABI -- with the document's
//! `/Lang` and its role map.
//!
//! Each function is one facade call (ruling 11). A [`TpdfTag`] is an owned
//! `Tag` built a property at a time; `open_tag` borrows it, so one tag may
//! open several elements. Where the facade answers `false` -- an element past
//! `MAX_TAG_DEPTH`, a close with nothing this call may close, a role mapping
//! it refuses -- the call is [`TpdfStatus::EditRefused`] naming why, and
//! nothing is written. A table's attributes, a namespace and associated
//! files on an element are not here: each is a shape of its own.

use std::ffi::c_char;

use tinker_pdf::Tag;

use crate::{
    builder_mut, page_mut, refused, required_bytes, required_str, set_error, TpdfBuilder,
    TpdfPageBuilder, TpdfStatus,
};

/// Which text property [`tpdf_tag_set_text`] sets (14.7.2 Table 323, 14.9).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfTagText {
    /// `/T`, the element's title.
    Title = 0,
    /// `/Lang`, the natural language of its content: a BCP 47 tag, or empty
    /// for unknown.
    Lang = 1,
    /// `/Alt`, a description of content that is not text.
    Alt = 2,
    /// `/ActualText`, what the content is where the glyphs do not say it.
    ActualText = 3,
    /// `/E`, the expansion of an abbreviation.
    Expansion = 4,
}

/// A structure element to open: its type and properties. Opaque.
pub struct TpdfTag {
    inner: Tag,
}

/// A live tag, or the refusal.
unsafe fn tag_mut<'a>(tag: *mut TpdfTag) -> Result<&'a mut TpdfTag, TpdfStatus> {
    match unsafe { tag.as_mut() } {
        Some(tag) => Ok(tag),
        None => {
            set_error("null tag");
            Err(TpdfStatus::BadArgument)
        }
    }
}

/// Applies one of `Tag`'s consuming builders in place.
fn rebuild(tag: &mut TpdfTag, with: impl FnOnce(Tag) -> Tag) {
    let taken = std::mem::replace(&mut tag.inner, Tag::new(b""));
    tag.inner = with(taken);
}

/// A structure element of type `kind` -- `P`, `H1`, `Figure`, `Span`, or a
/// type of the caller's own mapped with [`tpdf_builder_map_role`] --
/// `Tag::new`. Freed with [`tpdf_tag_free`].
///
/// # Safety
///
/// `kind` must be valid for `kind_len` bytes and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_tag_new(
    kind: *const u8,
    kind_len: usize,
    out: *mut *mut TpdfTag,
) -> TpdfStatus {
    let kind = match unsafe { required_bytes(kind, kind_len, "structure type") } {
        Ok(kind) => kind,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = Box::into_raw(Box::new(TpdfTag {
        inner: Tag::new(kind),
    }));
    TpdfStatus::Ok
}

/// Sets one of the element's text properties -- `Tag::title`, `lang`,
/// `alt`, `actual_text` or `expansion`, as `which` names. Setting one twice
/// keeps the second.
///
/// # Safety
///
/// `tag` must be a live handle and `text` null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn tpdf_tag_set_text(
    tag: *mut TpdfTag,
    which: TpdfTagText,
    text: *const c_char,
) -> TpdfStatus {
    let tag = match unsafe { tag_mut(tag) } {
        Ok(tag) => tag,
        Err(status) => return status,
    };
    let text = match unsafe { required_str(text, "text") } {
        Ok(text) => text,
        Err(status) => return status,
    };
    rebuild(tag, |inner| match which {
        TpdfTagText::Title => inner.title(&text),
        TpdfTagText::Lang => inner.lang(&text),
        TpdfTagText::Alt => inner.alt(&text),
        TpdfTagText::ActualText => inner.actual_text(&text),
        TpdfTagText::Expansion => inner.expansion(&text),
    });
    TpdfStatus::Ok
}

/// Sets `/ID`, the element's identifier -- `Tag::id`. The first element in
/// the tree's order to carry an identifier keeps it.
///
/// # Safety
///
/// `tag` must be a live handle and `id` valid for `id_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn tpdf_tag_set_id(
    tag: *mut TpdfTag,
    id: *const u8,
    id_len: usize,
) -> TpdfStatus {
    let tag = match unsafe { tag_mut(tag) } {
        Ok(tag) => tag,
        Err(status) => return status,
    };
    let id = match unsafe { required_bytes(id, id_len, "identifier") } {
        Ok(id) => id.to_vec(),
        Err(status) => return status,
    };
    rebuild(tag, |inner| inner.id(&id));
    TpdfStatus::Ok
}

/// Names the element, so that halves drawn apart are one element --
/// `Tag::keyed`: `key` says which element, `order` where this half reads.
///
/// # Safety
///
/// `tag` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_tag_set_key(tag: *mut TpdfTag, key: u64, order: u64) -> TpdfStatus {
    match unsafe { tag_mut(tag) } {
        Ok(tag) => {
            rebuild(tag, |inner| inner.keyed(key, order));
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// Writes the element even if nothing is drawn inside it --
/// `Tag::keep_empty`.
///
/// # Safety
///
/// `tag` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_tag_keep_empty(tag: *mut TpdfTag) -> TpdfStatus {
    match unsafe { tag_mut(tag) } {
        Ok(tag) => {
            rebuild(tag, Tag::keep_empty);
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// Frees a tag. Null is accepted and does nothing. An element it opened
/// stays open: the page holds its own copy.
///
/// # Safety
///
/// `tag` must have come from [`tpdf_tag_new`] and must not be used
/// afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_tag_free(tag: *mut TpdfTag) {
    if !tag.is_null() {
        drop(unsafe { Box::from_raw(tag) });
    }
}

/// Opens the element `tag` describes, so everything drawn until the
/// matching [`tpdf_page_builder_close_tag`] belongs to it -- across drawing
/// calls and across pages (an element open when its page is pushed is
/// reopened on the next page begun) -- `PageBuilder::open_tag`.
///
/// [`TpdfStatus::EditRefused`], opening nothing, past `MAX_TAG_DEPTH`
/// nested elements; what is drawn then belongs to the element around it,
/// and the matching close is still owed.
///
/// # Safety
///
/// `page` and `tag` must be live handles.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_open_tag(
    page: *mut TpdfPageBuilder,
    tag: *const TpdfTag,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "open_tag") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    let Some(tag) = (unsafe { tag.as_ref() }) else {
        set_error("null tag");
        return TpdfStatus::BadArgument;
    };
    if page.open_tag(&tag.inner) {
        TpdfStatus::Ok
    } else {
        refused(
            "open_tag",
            &format!(
                "{:?} is past the deepest nesting this engine reads back",
                String::from_utf8_lossy(tag.inner.kind())
            ),
        )
    }
}

/// Closes the innermost element [`tpdf_page_builder_open_tag`] opened --
/// `PageBuilder::close_tag`. [`TpdfStatus::EditRefused`], closing nothing,
/// when no element is open; a close matching a refused open is accepted.
///
/// # Safety
///
/// `page` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_close_tag(page: *mut TpdfPageBuilder) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "close_tag") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    if page.close_tag() {
        TpdfStatus::Ok
    } else {
        refused("close_tag", "no element is open")
    }
}

/// Sets the document's natural language, the catalog's `/Lang` --
/// `DocumentBuilder::set_language`: a BCP 47 tag, or empty for unknown.
///
/// # Safety
///
/// `builder` must be a live handle and `language` null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_set_language(
    builder: *mut TpdfBuilder,
    language: *const c_char,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "set_language") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    match unsafe { required_str(language, "language") } {
        Ok(language) => {
            builder.set_language(&language);
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// Maps a structure type of the caller's own to a standard one in the
/// `/RoleMap` -- `DocumentBuilder::map_role`. [`TpdfStatus::EditRefused`],
/// mapping nothing, for an empty or identical pair, a type already mapped
/// elsewhere, a loop, or a full map.
///
/// # Safety
///
/// `builder` must be a live handle and both names valid for their lengths.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_map_role(
    builder: *mut TpdfBuilder,
    custom: *const u8,
    custom_len: usize,
    standard: *const u8,
    standard_len: usize,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "map_role") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let (Ok(custom), Ok(standard)) = (
        unsafe { required_bytes(custom, custom_len, "custom type") },
        unsafe { required_bytes(standard, standard_len, "standard type") },
    ) else {
        return TpdfStatus::BadArgument;
    };
    if builder.map_role(custom, standard) {
        TpdfStatus::Ok
    } else {
        refused(
            "map_role",
            &format!(
                "{:?} to {:?}",
                String::from_utf8_lossy(custom),
                String::from_utf8_lossy(standard)
            ),
        )
    }
}

#[cfg(test)]
mod tests;
