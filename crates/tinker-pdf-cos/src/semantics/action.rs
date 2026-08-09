//! Actions (12.6) and file specifications (7.11.3).
//!
//! Actions are **reported, never executed** — not in this phase and not in any
//! other. `/Launch` doubly so: its target is a string handed to the caller, and
//! nothing in this engine will ever spawn it. Treating an action as anything
//! but data about the document is a vulnerability, not a feature.

use crate::doc::CosDocument;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::semantics::dest::Destination;
use crate::semantics::text::decode_text_string;
use crate::warn::{WarningKind, WarningSink};

/// A file specification (7.11.3), with both spellings of the name kept raw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileSpec {
    /// `/F`, the platform-independent file name, as written.
    pub file: Option<Vec<u8>>,
    /// `/UF`, the Unicode file name, decoded as a text string (7.9.2.2).
    pub unicode_file: Option<String>,
    /// `/Desc`, decoded as a text string.
    pub description: Option<String>,
    /// The embedded stream `/EF` names, when it is an indirect reference. Read
    /// its bytes with the stream tiers on [`CosDocument`].
    pub embedded: Option<ObjRef>,
}

impl FileSpec {
    /// The name to show a user: `/UF` when it is there, otherwise `/F` decoded.
    ///
    /// 7.11.3 makes `/UF` preferred precisely because `/F` predates any
    /// agreement on encoding.
    pub fn display_name(&self) -> Option<String> {
        match &self.unicode_file {
            Some(name) => Some(name.clone()),
            None => self.file.as_deref().map(decode_text_string),
        }
    }
}

/// An action (12.6.4), as data.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// 12.6.4.2 `/GoTo`: a destination in this document.
    GoTo(Destination),
    /// 12.6.4.3 `/GoToR`: a destination in another document.
    GoToR {
        /// Which document.
        file: FileSpec,
        /// Where in it, when the action says.
        dest: Option<Destination>,
    },
    /// 12.6.4.7 `/URI`. The bytes as written: a URI is not a text string.
    Uri(Vec<u8>),
    /// 12.6.4.11 `/Named`: `/NextPage`, `/PrevPage`, `/FirstPage`, `/LastPage`.
    Named(Vec<u8>),
    /// 12.6.4.5 `/Launch`. Reported so a caller can show it and refuse it.
    Launch {
        /// What the document asks to be launched, when it names a file.
        target: Option<FileSpec>,
    },
    /// Any other `/S` — `/JavaScript`, `/SubmitForm`, `/Thread`, … — preserved
    /// by subtype rather than dropped, so an unrecognized action is visible
    /// instead of silently absent.
    Other {
        /// The `/S` name, as bytes.
        subtype: Vec<u8>,
    },
}

/// What an outline entry or a link points at.
///
/// `/Dest` and a `/GoTo` action collapse into [`Target::Dest`] because 12.3.2
/// defines them as equivalent, and a `/URI` action becomes
/// [`Destination::Uri`] because that is what it is. Consequently
/// [`Action::GoTo`] and [`Action::Uri`] never appear inside
/// [`Target::Action`]; they exist for callers parsing an action dictionary
/// directly.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A destination, in any of its three forms.
    Dest(Destination),
    /// Anything else the document wanted to happen.
    Action(Action),
}

impl CosDocument {
    /// Reads an action dictionary (12.6.2).
    ///
    /// `/Next` chains are not followed: only a scripting-capable viewer can act
    /// on them, so the first action is modeled and the presence of a chain is
    /// warned about.
    pub fn action(&self, dict: &Dict) -> Option<Action> {
        let mut sink = WarningSink::new();
        let action = self.action_in(dict, &mut sink);
        self.absorb(sink);
        action
    }

    pub(crate) fn action_in(&self, dict: &Dict, sink: &mut WarningSink) -> Option<Action> {
        let sem = &self.names.sem;
        if dict.contains_key(sem.next) {
            sink.warn(0, WarningKind::ActionChainIgnored);
        }
        let subtype = dict.get_name(sem.s)?;
        if subtype == sem.go_to {
            let dest = dict.get(sem.d)?;
            return self.destination_in(dest, 0, sink).map(Action::GoTo);
        }
        if subtype == sem.go_to_r {
            let file = self.file_spec(dict.get(sem.f)?);
            let dest = dict
                .get(sem.d)
                .and_then(|object| self.destination_in(object, 0, sink));
            return Some(Action::GoToR { file, dest });
        }
        if subtype == sem.uri {
            let uri = self.resolve(dict.get(sem.uri)?);
            return Some(Action::Uri(uri.as_string()?.bytes.clone()));
        }
        if subtype == sem.named {
            let name = dict.get_name(Name::N)?;
            return Some(Action::Named(self.name_bytes(name)?.to_vec()));
        }
        if subtype == sem.launch {
            // 12.6.4.5: /F is the file, and the platform-specific /Win, /Mac
            // and /Unix entries are alternatives to it. Either way this engine
            // only ever reports them.
            let target = dict.get(sem.f).map(|object| self.file_spec(object));
            return Some(Action::Launch { target });
        }
        Some(Action::Other {
            subtype: self.name_bytes(subtype)?.to_vec(),
        })
    }

    /// Reads a file specification (7.11.3), which may be a plain string.
    pub fn file_spec(&self, object: &Object) -> FileSpec {
        let mut sink = WarningSink::new();
        let spec = self.file_spec_in(object, &mut sink);
        self.absorb(sink);
        spec
    }

    pub(crate) fn file_spec_in(&self, object: &Object, sink: &mut WarningSink) -> FileSpec {
        let sem = &self.names.sem;
        let resolved = self.resolve(object);
        // 7.11.2: a file specification may be written as a plain string.
        if let Some(s) = resolved.as_string() {
            return FileSpec {
                file: Some(s.bytes.clone()),
                ..FileSpec::default()
            };
        }
        let Some(dict) = resolved.as_dict() else {
            return FileSpec::default();
        };
        let text = |key: Name| -> Option<String> {
            let value = self.resolve(dict.get(key)?);
            let bytes = &value.as_string()?.bytes;
            Some(self.text_string(bytes, sink))
        };
        let ef = self.resolve_key(dict, sem.ef);
        let embedded = ef
            .as_dict()
            .and_then(|ef| ef.get_ref(sem.f).or_else(|| ef.get_ref(sem.uf)));
        FileSpec {
            file: self
                .resolve(dict.get(sem.f).unwrap_or(&Object::Null))
                .as_string()
                .map(|s| s.bytes.clone()),
            unicode_file: text(sem.uf),
            description: text(sem.desc),
            embedded,
        }
    }

    /// Reads the `/Dest` or `/A` of an outline entry or a link annotation.
    pub(crate) fn target_in(&self, dict: &Dict, sink: &mut WarningSink) -> Option<Target> {
        let sem = &self.names.sem;
        if let Some(dest) = dict.get(sem.dest) {
            if let Some(dest) = self.destination_in(dest, 0, sink) {
                return Some(Target::Dest(dest));
            }
        }
        let action = self.resolve_key(dict, sem.a);
        let action_dict = action.as_dict()?;
        match self.action_in(action_dict, sink)? {
            // 12.3.2: a /GoTo action and a /Dest say the same thing.
            Action::GoTo(dest) => Some(Target::Dest(dest)),
            // Ruling 6: a URI is a URI. It never becomes a name.
            Action::Uri(uri) => Some(Target::Dest(Destination::Uri(uri))),
            other => Some(Target::Action(other)),
        }
    }

    /// The destination a target carries, resolved to a page index.
    ///
    /// `None` when the target is an action, a URI, or a destination that
    /// resolves to nothing.
    pub fn resolve_target(&self, target: &Target) -> Option<crate::semantics::dest::ResolvedDest> {
        match target {
            Target::Dest(dest) => self.resolve_destination(dest),
            Target::Action(Action::GoTo(dest)) => self.resolve_destination(dest),
            Target::Action(_) => None,
        }
    }
}
