//! The form-script lexer, parser and evaluator.
//!
//! A calculate action's source is attacker-controlled text that arrives inside
//! an otherwise sound file, so it is the same class of input as a content
//! stream and gets the same treatment: never a panic, never an unbounded run
//! (ruling 1).
//!
//! Two properties are checked rather than one. The first is the usual
//! no-panic. The second is the one this target exists for: **the budget is
//! honoured**. A run is given a small budget and must come back having spent
//! no more than it, so a path that forgot to charge a step — which is how a
//! bounded interpreter stops being bounded — fails the target instead of
//! merely running slowly and being blamed on the corpus.
//!
//! # Every entry point the policy gates, from one input
//!
//! A bound that holds for `run` and not for a helper's body is not a bound, so
//! all of them are driven. The input is split at the first NUL byte: the
//! prefix is offered to `ScriptScope::define` as **document scope** (7.7.4),
//! and the suffix is run as a field script with whatever that produced in
//! scope. A seed with no NUL defines nothing and runs whole, which is what
//! every committed seed already did.
//!
//! The suffix is then run twice more, once as each event, because a keystroke
//! action reads members — `event.change`, the selection, `event.willCommit` —
//! that no calculate action ever touches, and the budget has to hold across
//! those paths too.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_cos::{Budget, Event, Host, ScriptError, ScriptScope};

/// A handful of fields, so `getField` and `AFSimple_Calculate` have somewhere
/// to land and the evaluator is exercised past the parser.
struct Fields {
    values: Vec<(&'static str, String)>,
}

impl Host for Fields {
    fn field(&self, name: &str) -> Option<String> {
        self.values
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
    }

    fn set_field(&mut self, name: &str, value: &str) -> bool {
        match self.values.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => {
                slot.1 = value.to_string();
                true
            }
            None => false,
        }
    }
}

fn fields() -> Fields {
    Fields {
        values: vec![
            ("a", "10".to_string()),
            ("b", "-2.5".to_string()),
            ("c", String::new()),
            ("total", "0".to_string()),
        ],
    }
}

/// Deliberately small. The point is not to let a script finish, it is to prove
/// the cap holds whatever the script does.
const LIMIT: u32 = 5_000;

/// Runs one entry point under a fresh budget and checks the budget survived.
fn bounded(what: &str, run: impl FnOnce(&mut Budget) -> Result<(), ScriptError>) {
    let mut budget = Budget::new(LIMIT);
    let outcome = run(&mut budget);
    assert!(
        budget.used() <= LIMIT,
        "{what}: the budget was overrun: {} of {LIMIT}",
        budget.used()
    );
    if let Err(ScriptError::OutOfSteps) = outcome {
        assert!(budget.is_spent(), "{what}: OutOfSteps with budget left");
    }
}

fuzz_target!(|data: &[u8]| {
    let (head, tail) = match data.iter().position(|b| *b == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (&data[..0], data),
    };

    // Source is text; invalid UTF-8 is the document's problem and lossy
    // decoding is what the field model does with a `/JS` string too.
    let definitions = String::from_utf8_lossy(head);
    let source = String::from_utf8_lossy(tail);

    // Document scope runs nothing, so it has no budget of its own. What it
    // must not do is panic, and what it must not build is a table the runs
    // below cannot then bound.
    let mut scope = ScriptScope::empty();
    let _ = scope.define(&definitions);

    let mut host = fields();
    bounded("run_in", |budget| {
        tinker_pdf_cos::script::run_in(&source, "total", "1234.5", &mut host, budget, &scope)
            .map(|_| ())
    });

    // A keystroke event, whose members no calculate action reaches.
    let event = Event {
        value: "1234.5".to_string(),
        change: "7".to_string(),
        selection: (1, 3),
        will_commit: true,
    };
    let mut host = fields();
    bounded("run_event keystroke", |budget| {
        tinker_pdf_cos::script::run_event(&source, "total", &event, &mut host, budget, &scope)
            .map(|_| ())
    });

    // And a validate event, which starts from a committed value and no change.
    let event = Event {
        value: source.chars().take(32).collect(),
        ..Event::default()
    };
    let mut host = fields();
    bounded("run_event validate", |budget| {
        tinker_pdf_cos::script::run_event(&source, "total", &event, &mut host, budget, &scope)
            .map(|_| ())
    });
});
