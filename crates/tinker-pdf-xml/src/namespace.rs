//! The namespace bindings in scope, by prefix.
//!
//! Namespaces in XML §6.1: a declaration's scope is the element it is on and
//! everything inside it, and an inner declaration of a prefix shadows an outer
//! one until the inner element closes. So each prefix keeps a stack of its
//! bindings, innermost last, and the scope as a whole keeps the order they
//! were made in, so that closing an element unbinds exactly what it bound and
//! a shadowed binding comes back.
//!
//! **A lookup is one probe of one prefix's stack**, however many other
//! bindings are in scope. A search of every binding in scope, innermost
//! first, cost each name as many steps as there were bindings declared inside
//! the one it found — and an element nested a few hundred deep under elements
//! declaring a few hundred prefixes each holds tens of thousands of bindings
//! in front of every unprefixed name, which looks up the default namespace
//! (`tests/complexity.rs` holds both shapes to the size-scaling law). Nothing
//! iterates the map, so the order its hasher gives it never reaches an answer.

use std::borrow::Cow;
use std::collections::HashMap;

/// Every namespace binding in scope.
#[derive(Debug, Default)]
pub(crate) struct Scope<'a> {
    /// Each prefix's bindings, innermost last. `""` is the default namespace,
    /// and a `None` URI is a prefix its innermost declaration undeclared.
    by_prefix: HashMap<&'a str, Vec<Option<Cow<'a, str>>>>,
    /// Every prefix bound, in the order its binding was made.
    order: Vec<&'a str>,
}

impl<'a> Scope<'a> {
    /// Binds `prefix` to `uri`, shadowing its outer bindings.
    pub(crate) fn bind(&mut self, prefix: &'a str, uri: Option<Cow<'a, str>>) {
        self.by_prefix.entry(prefix).or_default().push(uri);
        self.order.push(prefix);
    }

    /// The innermost binding of `prefix`: `None` when it was never bound, or
    /// when its innermost declaration undeclared it.
    pub(crate) fn lookup(&self, prefix: &str) -> Option<Cow<'a, str>> {
        self.by_prefix.get(prefix)?.last()?.clone()
    }

    /// Unbinds the last `count` bindings made, which brings back whatever
    /// each of them shadowed.
    pub(crate) fn unbind(&mut self, count: usize) {
        for _ in 0..count {
            let Some(prefix) = self.order.pop() else {
                return;
            };
            if let Some(stack) = self.by_prefix.get_mut(prefix) {
                stack.pop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inner_binding_shadows_an_outer_one_until_it_is_unbound() {
        let mut scope = Scope::default();
        scope.bind("p", Some(Cow::Borrowed("urn:outer")));
        scope.bind("", Some(Cow::Borrowed("urn:default")));
        assert_eq!(scope.lookup("p").as_deref(), Some("urn:outer"));
        scope.bind("p", Some(Cow::Borrowed("urn:inner")));
        assert_eq!(scope.lookup("p").as_deref(), Some("urn:inner"));
        assert_eq!(scope.lookup("").as_deref(), Some("urn:default"));
        scope.unbind(1);
        assert_eq!(scope.lookup("p").as_deref(), Some("urn:outer"));
        scope.unbind(2);
        assert_eq!(scope.lookup("p"), None);
        assert_eq!(scope.lookup(""), None);
    }

    #[test]
    fn an_undeclared_prefix_looks_up_as_unbound_and_comes_back_when_closed() {
        let mut scope = Scope::default();
        scope.bind("p", Some(Cow::Borrowed("urn:a")));
        scope.bind("p", None);
        assert_eq!(scope.lookup("p"), None);
        scope.unbind(1);
        assert_eq!(scope.lookup("p").as_deref(), Some("urn:a"));
    }

    #[test]
    fn one_element_binding_one_prefix_twice_unbinds_both() {
        // `xmlns="a" xmlns:="b"` both bind the default namespace.
        let mut scope = Scope::default();
        scope.bind("", Some(Cow::Borrowed("urn:root")));
        scope.bind("", Some(Cow::Borrowed("urn:a")));
        scope.bind("", Some(Cow::Borrowed("urn:b")));
        assert_eq!(scope.lookup("").as_deref(), Some("urn:b"));
        scope.unbind(2);
        assert_eq!(scope.lookup("").as_deref(), Some("urn:root"));
    }

    #[test]
    fn unbinding_more_than_was_bound_stops_at_empty() {
        let mut scope = Scope::default();
        scope.bind("q", Some(Cow::Borrowed("urn:q")));
        scope.unbind(5);
        assert_eq!(scope.lookup("q"), None);
        scope.bind("q", Some(Cow::Borrowed("urn:again")));
        assert_eq!(scope.lookup("q").as_deref(), Some("urn:again"));
    }
}
